use std::fmt;
use std::ops::Deref;

use actix_http::dev::Payload;
use actix_http::http::HeaderMap;
use actix_http::Request as BaseRequest;
use actix_http::{Error, HttpMessage};
use futures::future::{ok, FutureResult};

use app::State;
use handler::FromRequest;
use param::Params;

pub struct Request<S = ()> {
    base: BaseRequest,
    params: Params,
    state: State<S>,
}

impl<S> Request<S> {
    #[inline]
    pub fn new(state: State<S>, base: BaseRequest, params: Params) -> Request<S> {
        Request {
            state,
            base,
            params,
        }
    }

    /// Construct new http request with empty state.
    pub fn drop_state(&self) -> Request<()> {
        Request {
            state: State::new(()),
            base: self.base.clone_request(),
            params: self.params.clone(),
        }
    }

    #[inline]
    /// Shared application state
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Returns shared application state
    pub fn get_state(&self) -> State<S> {
        self.state.clone()
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

impl<S> Clone for Request<S> {
    fn clone(&self) -> Request<S> {
        Request {
            base: self.base.clone_request(),
            params: self.params.clone(),
            state: self.state.clone(),
        }
    }
}

impl<S> Deref for Request<S> {
    type Target = BaseRequest;

    fn deref(&self) -> &BaseRequest {
        self.request()
    }
}

impl<S> HttpMessage for Request<S> {
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

impl<S> FromRequest<S> for Request<S> {
    type Config = ();
    type Error = Error;
    type Future = FutureResult<Self, Error>;

    #[inline]
    fn from_request(req: &Request<S>, _: &Self::Config) -> Self::Future {
        ok(req.clone())
    }
}

impl<S> fmt::Debug for Request<S> {
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
