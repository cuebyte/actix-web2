use std::fmt;
use std::ops::Deref;

use actix_http::dev::Payload;
use actix_http::http::HeaderMap;
use actix_http::HttpMessage;
use actix_http::Request as BaseRequest;

use handler::FromRequest;
use param::Params;

pub struct Request {
    base: BaseRequest,
    params: Params,
}

impl Request {
    #[inline]
    pub fn new(base: BaseRequest, params: Params) -> Request {
        Request { base, params }
    }

    /// This method returns reference to current base `Request` object.
    #[inline]
    pub fn request(&self) -> &BaseRequest {
        &self.base
    }

    /// The query string in the URL.
    ///
    /// E.g., id=10
    #[inline]
    pub fn query_string(&self) -> &str {
        if let Some(query) = self.uri().query().as_ref() {
            query
        } else {
            ""
        }
    }

    /// Get a reference to the Params object.
    ///
    /// Params is a container for url parameters.
    /// A variable segment is specified in the form `{identifier}`,
    /// where the identifier can be used later in a request handler to
    /// access the matched value for that segment.
    #[inline]
    pub fn match_info(&self) -> &Params {
        &self.params
    }
}

impl Clone for Request {
    fn clone(&self) -> Request {
        Request {
            base: self.base.clone_request(),
            params: self.params.clone(),
        }
    }
}

impl Deref for Request {
    type Target = BaseRequest;

    fn deref(&self) -> &BaseRequest {
        self.request()
    }
}

impl HttpMessage for Request {
    type Stream = Payload;

    #[inline]
    fn headers(&self) -> &HeaderMap {
        self.request().headers()
    }

    #[inline]
    fn payload(&self) -> Payload {
        self.request().payload()
    }
}

impl FromRequest for Request {
    type Config = ();
    type Result = Self;

    #[inline]
    fn from_request(req: &Request, _: &Self::Config) -> Self::Result {
        req.clone()
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            f,
            "\nRequest {:?} {}:{}",
            self.version(),
            self.method(),
            self.path()
        )?;
        if !self.query_string().is_empty() {
            writeln!(f, "  query: ?{:?}", self.query_string())?;
        }
        if !self.match_info().is_empty() {
            writeln!(f, "  params: {:?}", self.match_info())?;
        }
        writeln!(f, "  headers:")?;
        for (key, val) in self.headers().iter() {
            writeln!(f, "    {:?}: {:?}", key, val)?;
        }
        Ok(())
    }
}
