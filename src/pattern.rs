use std::cmp::min;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use actix_http::Request;
use regex::{escape, Regex};

use super::param::{ParamItem, Params};

#[derive(Debug, Clone, PartialEq)]
enum PatternElement {
    Str(String),
    Var(String),
}

#[derive(Clone, Debug)]
enum PatternType {
    Static(String),
    Prefix(String),
    Dynamic(Regex, Vec<Rc<String>>, usize),
}

#[derive(Debug, Copy, Clone, PartialEq)]
/// Resource type
pub enum ResourceType {
    /// Normal resource
    Normal,
    /// Resource for application default handler
    Default,
    /// External resource
    External,
    /// Unknown resource type
    Unset,
}

/// Resource type describes an entry in resources table
#[derive(Clone, Debug)]
pub struct ResourcePattern {
    tp: PatternType,
    rtp: ResourceType,
    pattern: String,
    elements: Vec<PatternElement>,
}

impl ResourcePattern {
    /// Parse path pattern and create new `ResourcePattern` instance.
    ///
    /// Panics if path pattern is wrong.
    pub fn new(path: &str) -> Self {
        ResourcePattern::with_prefix(path, false, !path.is_empty())
    }

    /// Parse path pattern and create new `ResourcePattern` instance.
    ///
    /// Use `prefix` type instead of `static`.
    ///
    /// Panics if path regex pattern is wrong.
    pub fn prefix(path: &str) -> Self {
        ResourcePattern::with_prefix(path, true, !path.is_empty())
    }

    /// Construct external resource def
    ///
    /// Panics if path pattern is wrong.
    pub fn external(path: &str) -> Self {
        let mut resource = ResourcePattern::with_prefix(path, false, false);
        resource.rtp = ResourceType::External;
        resource
    }

    /// Parse path pattern and create new `ResourcePattern` instance with custom prefix
    pub fn with_prefix(path: &str, for_prefix: bool, slash: bool) -> Self {
        let mut path = path.to_owned();
        if slash && !path.starts_with('/') {
            path.insert(0, '/');
        }
        let (pattern, elements, is_dynamic, len) =
            ResourcePattern::parse(&path, for_prefix);

        let tp = if is_dynamic {
            let re = match Regex::new(&pattern) {
                Ok(re) => re,
                Err(err) => panic!("Wrong path pattern: \"{}\" {}", path, err),
            };
            // actix creates one router per thread
            let names = re
                .capture_names()
                .filter_map(|name| name.map(|name| Rc::new(name.to_owned())))
                .collect();
            PatternType::Dynamic(re, names, len)
        } else if for_prefix {
            PatternType::Prefix(pattern.clone())
        } else {
            PatternType::Static(pattern.clone())
        };

        ResourcePattern {
            tp,
            elements,
            rtp: ResourceType::Normal,
            pattern: path.to_owned(),
        }
    }

    /// Resource type
    pub fn rtype(&self) -> ResourceType {
        self.rtp
    }

    /// Path pattern of the resource
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Is this path a match against this resource?
    pub fn is_match(&self, path: &str) -> bool {
        match self.tp {
            PatternType::Static(ref s) => s == path,
            PatternType::Dynamic(ref re, _, _) => re.is_match(path),
            PatternType::Prefix(ref s) => path.starts_with(s),
        }
    }

    fn is_prefix_match(&self, path: &str) -> Option<usize> {
        let plen = path.len();
        let path = if path.is_empty() { "/" } else { path };

        match self.tp {
            PatternType::Static(ref s) => {
                if s == path {
                    Some(plen)
                } else {
                    None
                }
            }
            PatternType::Dynamic(ref re, _, len) => {
                if let Some(captures) = re.captures(path) {
                    let mut pos = 0;
                    let mut passed = false;
                    for capture in captures.iter() {
                        if let Some(ref m) = capture {
                            if !passed {
                                passed = true;
                                continue;
                            }

                            pos = m.end();
                        }
                    }
                    Some(plen + pos + len)
                } else {
                    None
                }
            }
            PatternType::Prefix(ref s) => {
                let len = if path == s {
                    s.len()
                } else if path.starts_with(s)
                    && (s.ends_with('/') || path.split_at(s.len()).1.starts_with('/'))
                {
                    if s.ends_with('/') {
                        s.len() - 1
                    } else {
                        s.len()
                    }
                } else {
                    return None;
                };
                Some(min(plen, len))
            }
        }
    }

    /// Are the given path and parameters a match against this resource?
    pub fn match_with_params(&self, req: &Request, plen: usize) -> Option<Params> {
        let path = &req.path()[plen..];

        match self.tp {
            PatternType::Static(ref s) => {
                if s != path {
                    None
                } else {
                    Some(Params::with_uri(req.uri()))
                }
            }
            PatternType::Dynamic(ref re, ref names, _) => {
                if let Some(captures) = re.captures(path) {
                    let mut params = Params::with_uri(req.uri());
                    let mut idx = 0;
                    let mut passed = false;
                    for capture in captures.iter() {
                        if let Some(ref m) = capture {
                            if !passed {
                                passed = true;
                                continue;
                            }
                            params.add(
                                names[idx].clone(),
                                ParamItem::UrlSegment(
                                    (plen + m.start()) as u16,
                                    (plen + m.end()) as u16,
                                ),
                            );
                            idx += 1;
                        }
                    }
                    params.set_tail(req.path().len() as u16);
                    Some(params)
                } else {
                    None
                }
            }
            PatternType::Prefix(ref s) => {
                if !path.starts_with(s) {
                    None
                } else {
                    Some(Params::with_uri(req.uri()))
                }
            }
        }
    }

    /// Is the given path a prefix match and do the parameters match against this resource?
    pub fn match_prefix_with_params(
        &self,
        req: &Request,
        plen: usize,
    ) -> Option<Params> {
        let path = &req.path()[plen..];
        let path = if path.is_empty() { "/" } else { path };

        match self.tp {
            PatternType::Static(ref s) => {
                if s == path {
                    let mut params = Params::with_uri(req.uri());
                    params.set_tail(req.path().len() as u16);
                    Some(params)
                } else {
                    None
                }
            }
            PatternType::Dynamic(ref re, ref names, len) => {
                if let Some(captures) = re.captures(path) {
                    let mut params = Params::with_uri(req.uri());
                    let mut pos = 0;
                    let mut passed = false;
                    let mut idx = 0;
                    for capture in captures.iter() {
                        if let Some(ref m) = capture {
                            if !passed {
                                passed = true;
                                continue;
                            }

                            params.add(
                                names[idx].clone(),
                                ParamItem::UrlSegment(
                                    (plen + m.start()) as u16,
                                    (plen + m.end()) as u16,
                                ),
                            );
                            idx += 1;
                            pos = m.end();
                        }
                    }
                    params.set_tail((plen + pos + len) as u16);
                    Some(params)
                } else {
                    None
                }
            }
            PatternType::Prefix(ref s) => {
                let len = if path == s {
                    s.len()
                } else if path.starts_with(s)
                    && (s.ends_with('/') || path.split_at(s.len()).1.starts_with('/'))
                {
                    if s.ends_with('/') {
                        s.len() - 1
                    } else {
                        s.len()
                    }
                } else {
                    return None;
                };
                let mut params = Params::with_uri(req.uri());
                params.set_tail(min(req.path().len(), plen + len) as u16);
                Some(params)
            }
        }
    }

    // /// Build resource path.
    // pub fn resource_path<U, I>(
    //     &self, path: &mut String, elements: &mut U,
    // ) -> Result<(), UrlGenerationError>
    // where
    //     U: Iterator<Item = I>,
    //     I: AsRef<str>,
    // {
    //     match self.tp {
    //         PatternType::Prefix(ref p) => path.push_str(p),
    //         PatternType::Static(ref p) => path.push_str(p),
    //         PatternType::Dynamic(..) => {
    //             for el in &self.elements {
    //                 match *el {
    //                     PatternElement::Str(ref s) => path.push_str(s),
    //                     PatternElement::Var(_) => {
    //                         if let Some(val) = elements.next() {
    //                             path.push_str(val.as_ref())
    //                         } else {
    //                             return Err(UrlGenerationError::NotEnoughElements);
    //                         }
    //                     }
    //                 }
    //             }
    //         }
    //     };
    //     Ok(())
    // }

    fn parse_param(pattern: &str) -> (PatternElement, String, &str) {
        const DEFAULT_PATTERN: &str = "[^/]+";
        let mut params_nesting = 0usize;
        let close_idx = pattern
            .find(|c| match c {
                '{' => {
                    params_nesting += 1;
                    false
                }
                '}' => {
                    params_nesting -= 1;
                    params_nesting == 0
                }
                _ => false,
            })
            .expect("malformed param");
        let (mut param, rem) = pattern.split_at(close_idx + 1);
        param = &param[1..param.len() - 1]; // Remove outer brackets
        let (name, pattern) = match param.find(':') {
            Some(idx) => {
                let (name, pattern) = param.split_at(idx);
                (name, &pattern[1..])
            }
            None => (param, DEFAULT_PATTERN),
        };
        (
            PatternElement::Var(name.to_string()),
            format!(r"(?P<{}>{})", &name, &pattern),
            rem,
        )
    }

    fn parse(
        mut pattern: &str,
        for_prefix: bool,
    ) -> (String, Vec<PatternElement>, bool, usize) {
        if pattern.find('{').is_none() {
            return (
                String::from(pattern),
                vec![PatternElement::Str(String::from(pattern))],
                false,
                pattern.chars().count(),
            );
        };

        let mut elems = Vec::new();
        let mut re = String::from("^");

        while let Some(idx) = pattern.find('{') {
            let (prefix, rem) = pattern.split_at(idx);
            elems.push(PatternElement::Str(String::from(prefix)));
            re.push_str(&escape(prefix));
            let (param_pattern, re_part, rem) = Self::parse_param(rem);
            elems.push(param_pattern);
            re.push_str(&re_part);
            pattern = rem;
        }

        elems.push(PatternElement::Str(String::from(pattern)));
        re.push_str(&escape(pattern));

        if !for_prefix {
            re.push_str("$");
        }

        (re, elems, true, pattern.chars().count())
    }
}

impl PartialEq for ResourcePattern {
    fn eq(&self, other: &ResourcePattern) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for ResourcePattern {}

impl Hash for ResourcePattern {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pattern.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test::TestRequest;

    #[test]
    fn test_recognizer10() {
        let mut router = Router::<()>::default();
        router.register_resource(Resource::new(ResourcePattern::new("/name")));
        router.register_resource(Resource::new(ResourcePattern::new("/name/{val}")));
        router.register_resource(Resource::new(ResourcePattern::new(
            "/name/{val}/index.html",
        )));
        router.register_resource(Resource::new(ResourcePattern::new(
            "/file/{file}.{ext}",
        )));
        router.register_resource(Resource::new(ResourcePattern::new(
            "/v{val}/{val2}/index.html",
        )));
        router.register_resource(Resource::new(ResourcePattern::new("/v/{tail:.*}")));
        router.register_resource(Resource::new(ResourcePattern::new(
            "/test2/{test}.html",
        )));
        router.register_resource(Resource::new(ResourcePattern::new(
            "/{test}/index.html",
        )));

        let req = TestRequest::with_uri("/name").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(0));
        assert!(info.match_info().is_empty());

        let req = TestRequest::with_uri("/name/value").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(1));
        assert_eq!(info.match_info().get("val").unwrap(), "value");
        assert_eq!(&info.match_info()["val"], "value");

        let req = TestRequest::with_uri("/name/value2/index.html").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(2));
        assert_eq!(info.match_info().get("val").unwrap(), "value2");

        let req = TestRequest::with_uri("/file/file.gz").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(3));
        assert_eq!(info.match_info().get("file").unwrap(), "file");
        assert_eq!(info.match_info().get("ext").unwrap(), "gz");

        let req = TestRequest::with_uri("/vtest/ttt/index.html").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(4));
        assert_eq!(info.match_info().get("val").unwrap(), "test");
        assert_eq!(info.match_info().get("val2").unwrap(), "ttt");

        let req = TestRequest::with_uri("/v/blah-blah/index.html").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(5));
        assert_eq!(
            info.match_info().get("tail").unwrap(),
            "blah-blah/index.html"
        );

        let req = TestRequest::with_uri("/test2/index.html").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(6));
        assert_eq!(info.match_info().get("test").unwrap(), "index");

        let req = TestRequest::with_uri("/bbb/index.html").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(7));
        assert_eq!(info.match_info().get("test").unwrap(), "bbb");
    }

    #[test]
    fn test_recognizer_2() {
        let mut router = Router::<()>::default();
        router.register_resource(Resource::new(ResourcePattern::new("/index.json")));
        router.register_resource(Resource::new(ResourcePattern::new("/{source}.json")));

        let req = TestRequest::with_uri("/index.json").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(0));

        let req = TestRequest::with_uri("/test.json").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(1));
    }

    #[test]
    fn test_recognizer_with_prefix() {
        let mut router = Router::<()>::default();
        router.register_resource(Resource::new(ResourcePattern::new("/name")));
        router.register_resource(Resource::new(ResourcePattern::new("/name/{val}")));

        let req = TestRequest::with_uri("/name").finish();
        let info = router.recognize(&req, &(), 5);
        assert_eq!(info.resource, ResourceId::Default);

        let req = TestRequest::with_uri("/test/name").finish();
        let info = router.recognize(&req, &(), 5);
        assert_eq!(info.resource, ResourceId::Normal(0));

        let req = TestRequest::with_uri("/test/name/value").finish();
        let info = router.recognize(&req, &(), 5);
        assert_eq!(info.resource, ResourceId::Normal(1));
        assert_eq!(info.match_info().get("val").unwrap(), "value");
        assert_eq!(&info.match_info()["val"], "value");

        // same patterns
        let mut router = Router::<()>::default();
        router.register_resource(Resource::new(ResourcePattern::new("/name")));
        router.register_resource(Resource::new(ResourcePattern::new("/name/{val}")));

        let req = TestRequest::with_uri("/name").finish();
        let info = router.recognize(&req, &(), 6);
        assert_eq!(info.resource, ResourceId::Default);

        let req = TestRequest::with_uri("/test2/name").finish();
        let info = router.recognize(&req, &(), 6);
        assert_eq!(info.resource, ResourceId::Normal(0));

        let req = TestRequest::with_uri("/test2/name-test").finish();
        let info = router.recognize(&req, &(), 6);
        assert_eq!(info.resource, ResourceId::Default);

        let req = TestRequest::with_uri("/test2/name/ttt").finish();
        let info = router.recognize(&req, &(), 6);
        assert_eq!(info.resource, ResourceId::Normal(1));
        assert_eq!(&info.match_info()["val"], "ttt");
    }

    #[test]
    fn test_parse_static() {
        let re = ResourcePattern::new("/");
        assert!(re.is_match("/"));
        assert!(!re.is_match("/a"));

        let re = ResourcePattern::new("/name");
        assert!(re.is_match("/name"));
        assert!(!re.is_match("/name1"));
        assert!(!re.is_match("/name/"));
        assert!(!re.is_match("/name~"));

        let re = ResourcePattern::new("/name/");
        assert!(re.is_match("/name/"));
        assert!(!re.is_match("/name"));
        assert!(!re.is_match("/name/gs"));

        let re = ResourcePattern::new("/user/profile");
        assert!(re.is_match("/user/profile"));
        assert!(!re.is_match("/user/profile/profile"));
    }

    #[test]
    fn test_parse_param() {
        let re = ResourcePattern::new("/user/{id}");
        assert!(re.is_match("/user/profile"));
        assert!(re.is_match("/user/2345"));
        assert!(!re.is_match("/user/2345/"));
        assert!(!re.is_match("/user/2345/sdg"));

        let req = TestRequest::with_uri("/user/profile").finish();
        let info = re.match_with_params(&req, 0).unwrap();
        assert_eq!(info.get("id").unwrap(), "profile");

        let req = TestRequest::with_uri("/user/1245125").finish();
        let info = re.match_with_params(&req, 0).unwrap();
        assert_eq!(info.get("id").unwrap(), "1245125");

        let re = ResourcePattern::new("/v{version}/resource/{id}");
        assert!(re.is_match("/v1/resource/320120"));
        assert!(!re.is_match("/v/resource/1"));
        assert!(!re.is_match("/resource"));

        let req = TestRequest::with_uri("/v151/resource/adahg32").finish();
        let info = re.match_with_params(&req, 0).unwrap();
        assert_eq!(info.get("version").unwrap(), "151");
        assert_eq!(info.get("id").unwrap(), "adahg32");

        let re = ResourcePattern::new("/{id:[[:digit:]]{6}}");
        assert!(re.is_match("/012345"));
        assert!(!re.is_match("/012"));
        assert!(!re.is_match("/01234567"));
        assert!(!re.is_match("/XXXXXX"));

        let req = TestRequest::with_uri("/012345").finish();
        let info = re.match_with_params(&req, 0).unwrap();
        assert_eq!(info.get("id").unwrap(), "012345");
    }

    #[test]
    fn test_resource_prefix() {
        let re = ResourcePattern::prefix("/name");
        assert!(re.is_match("/name"));
        assert!(re.is_match("/name/"));
        assert!(re.is_match("/name/test/test"));
        assert!(re.is_match("/name1"));
        assert!(re.is_match("/name~"));

        let re = ResourcePattern::prefix("/name/");
        assert!(re.is_match("/name/"));
        assert!(re.is_match("/name/gs"));
        assert!(!re.is_match("/name"));
    }

    #[test]
    fn test_reousrce_prefix_dynamic() {
        let re = ResourcePattern::prefix("/{name}/");
        assert!(re.is_match("/name/"));
        assert!(re.is_match("/name/gs"));
        assert!(!re.is_match("/name"));

        let req = TestRequest::with_uri("/test2/").finish();
        let info = re.match_with_params(&req, 0).unwrap();
        assert_eq!(&info["name"], "test2");
        assert_eq!(&info[0], "test2");

        let req = TestRequest::with_uri("/test2/subpath1/subpath2/index.html").finish();
        let info = re.match_with_params(&req, 0).unwrap();
        assert_eq!(&info["name"], "test2");
        assert_eq!(&info[0], "test2");
    }

    #[test]
    fn test_request_resource() {
        let mut router = Router::<()>::default();
        let mut resource = Resource::new(ResourcePattern::new("/index.json"));
        resource.name("r1");
        router.register_resource(resource);
        let mut resource = Resource::new(ResourcePattern::new("/test.json"));
        resource.name("r2");
        router.register_resource(resource);

        let req = TestRequest::with_uri("/index.json").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(0));

        assert_eq!(info.name(), "r1");

        let req = TestRequest::with_uri("/test.json").finish();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(1));
        assert_eq!(info.name(), "r2");
    }

    #[test]
    fn test_has_resource() {
        let mut router = Router::<()>::default();
        let scope = Scope::new("/test").resource("/name", |_| "done");
        router.register_scope(scope);

        {
            let info = router.default_route_info();
            assert!(!info.has_resource("/test"));
            assert!(info.has_resource("/test/name"));
        }

        let scope =
            Scope::new("/test2").nested("/test10", |s| s.resource("/name", |_| "done"));
        router.register_scope(scope);

        let info = router.default_route_info();
        assert!(info.has_resource("/test2/test10/name"));
    }

    #[test]
    fn test_url_for() {
        let mut router = Router::<()>::new(ResourcePattern::prefix(""));

        let mut resource = Resource::new(ResourcePattern::new("/tttt"));
        resource.name("r0");
        router.register_resource(resource);

        let scope = Scope::new("/test").resource("/name", |r| {
            r.name("r1");
        });
        router.register_scope(scope);

        let scope = Scope::new("/test2")
            .nested("/test10", |s| s.resource("/name", |r| r.name("r2")));
        router.register_scope(scope);
        router.finish();

        let req = TestRequest::with_uri("/test").request();
        {
            let info = router.default_route_info();

            let res = info
                .url_for(&req, "r0", Vec::<&'static str>::new())
                .unwrap();
            assert_eq!(res.as_str(), "http://localhost:8080/tttt");

            let res = info
                .url_for(&req, "r1", Vec::<&'static str>::new())
                .unwrap();
            assert_eq!(res.as_str(), "http://localhost:8080/test/name");

            let res = info
                .url_for(&req, "r2", Vec::<&'static str>::new())
                .unwrap();
            assert_eq!(res.as_str(), "http://localhost:8080/test2/test10/name");
        }

        let req = TestRequest::with_uri("/test/name").request();
        let info = router.recognize(&req, &(), 0);
        assert_eq!(info.resource, ResourceId::Normal(1));

        let res = info
            .url_for(&req, "r0", Vec::<&'static str>::new())
            .unwrap();
        assert_eq!(res.as_str(), "http://localhost:8080/tttt");

        let res = info
            .url_for(&req, "r1", Vec::<&'static str>::new())
            .unwrap();
        assert_eq!(res.as_str(), "http://localhost:8080/test/name");

        let res = info
            .url_for(&req, "r2", Vec::<&'static str>::new())
            .unwrap();
        assert_eq!(res.as_str(), "http://localhost:8080/test2/test10/name");
    }

    #[test]
    fn test_url_for_dynamic() {
        let mut router = Router::<()>::new(ResourcePattern::prefix(""));

        let mut resource =
            Resource::new(ResourcePattern::new("/{name}/test/index.{ext}"));
        resource.name("r0");
        router.register_resource(resource);

        let scope = Scope::new("/{name1}").nested("/{name2}", |s| {
            s.resource("/{name3}/test/index.{ext}", |r| r.name("r2"))
        });
        router.register_scope(scope);
        router.finish();

        let req = TestRequest::with_uri("/test").request();
        {
            let info = router.default_route_info();

            let res = info.url_for(&req, "r0", vec!["sec1", "html"]).unwrap();
            assert_eq!(res.as_str(), "http://localhost:8080/sec1/test/index.html");

            let res = info
                .url_for(&req, "r2", vec!["sec1", "sec2", "sec3", "html"])
                .unwrap();
            assert_eq!(
                res.as_str(),
                "http://localhost:8080/sec1/sec2/sec3/test/index.html"
            );
        }
    }
}
