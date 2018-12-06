#![allow(dead_code)]
use std::ops::Index;
use std::rc::Rc;

use actix_http::http::Uri;

use crate::url::Url;

#[derive(Debug, Clone)]
pub(crate) enum ParamItem {
    Static(&'static str),
    UrlSegment(u16, u16),
}

/// Route match information
///
/// If resource path contains variable patterns, `Params` stores this variables.
#[derive(Debug, Clone)]
pub struct Params {
    url: Url,
    pub(crate) tail: u16,
    pub(crate) segments: Vec<(Rc<String>, ParamItem)>,
}

impl Params {
    pub(crate) fn new() -> Params {
        Params {
            url: Url::default(),
            tail: 0,
            segments: Vec::new(),
        }
    }

    pub(crate) fn with_uri(uri: &Uri) -> Params {
        Params {
            url: Url::new(uri.clone()),
            tail: 0,
            segments: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.segments.clear();
    }

    pub(crate) fn set_tail(&mut self, tail: u16) {
        self.tail = tail;
    }

    pub(crate) fn set_uri(&mut self, uri: &Uri) {
        self.url.update(uri);
    }

    pub(crate) fn add(&mut self, name: Rc<String>, value: ParamItem) {
        self.segments.push((name, value));
    }

    pub(crate) fn add_static(&mut self, name: &str, value: &'static str) {
        self.segments
            .push((Rc::new(name.to_string()), ParamItem::Static(value)));
    }

    /// Check if there are any matched patterns
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Check number of extracted parameters
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Get matched parameter by name without type conversion
    pub fn get(&self, key: &str) -> Option<&str> {
        for item in self.segments.iter() {
            if key == item.0.as_str() {
                return match item.1 {
                    ParamItem::Static(ref s) => Some(&s),
                    ParamItem::UrlSegment(s, e) => {
                        Some(&self.url.path()[(s as usize)..(e as usize)])
                    }
                };
            }
        }
        if key == "tail" {
            Some(&self.url.path()[(self.tail as usize)..])
        } else {
            None
        }
    }

    /// Get unprocessed part of path
    pub fn unprocessed(&self) -> &str {
        &self.url.path()[(self.tail as usize)..]
    }

    /// Get matched parameter by name.
    ///
    /// If keyed parameter is not available empty string is used as default
    /// value.
    pub fn query(&self, key: &str) -> &str {
        if let Some(s) = self.get(key) {
            s
        } else {
            ""
        }
    }

    /// Return iterator to items in parameter container
    pub fn iter(&self) -> ParamsIter {
        ParamsIter {
            idx: 0,
            params: self,
        }
    }
}

#[derive(Debug)]
pub struct ParamsIter<'a> {
    idx: usize,
    params: &'a Params,
}

impl<'a> Iterator for ParamsIter<'a> {
    type Item = (&'a str, &'a str);

    #[inline]
    fn next(&mut self) -> Option<(&'a str, &'a str)> {
        if self.idx < self.params.len() {
            let idx = self.idx;
            let res = match self.params.segments[idx].1 {
                ParamItem::Static(ref s) => &s,
                ParamItem::UrlSegment(s, e) => {
                    &self.params.url.path()[(s as usize)..(e as usize)]
                }
            };
            self.idx += 1;
            return Some((&self.params.segments[idx].0, res));
        }
        None
    }
}

impl<'a> Index<&'a str> for Params {
    type Output = str;

    fn index(&self, name: &'a str) -> &str {
        self.get(name)
            .expect("Value for parameter is not available")
    }
}

impl Index<usize> for Params {
    type Output = str;

    fn index(&self, idx: usize) -> &str {
        match self.segments[idx].1 {
            ParamItem::Static(ref s) => &s,
            ParamItem::UrlSegment(s, e) => &self.url.path()[(s as usize)..(e as usize)],
        }
    }
}
