use std::cell::{Ref, RefCell, RefMut};
use std::fmt;
use std::ops::Deref;
use std::rc::Rc;

use actix_http::http::{HeaderMap, Method, Uri, Version};
use actix_http::payload::Payload;
use actix_http::{
    Error, Extensions, HttpMessage, Message, Request as BaseRequest, RequestHead,
};
use actix_router::{Path, Url};
use futures::future::{ok, FutureResult};

use crate::app::State;
use crate::handler::FromRequest;

pub struct Request<S = ()> {
    head: Message<RequestHead>,
    path: Path<Url>,
    state: State<S>,
    payload: Option<Rc<RefCell<Option<Payload>>>>,
}

impl<S> Request<S> {
    #[inline]
    pub fn new(state: State<S>, path: Path<Url>, request: BaseRequest) -> Request<S> {
        let (head, payload) = request.into_parts();
        Request {
            head,
            path,
            state,
            payload: payload.map(|p| Rc::new(RefCell::new(Some(p)))),
        }
    }

    /// Construct new http request with empty state.
    pub fn drop_state(self) -> Request<()> {
        Request {
            state: State::new(()),
            head: self.head,
            path: self.path,
            payload: self.payload,
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

    /// This method returns reference to the request head
    #[inline]
    pub fn head(&self) -> &RequestHead {
        &self.head
    }

    /// Request's uri.
    #[inline]
    pub fn uri(&self) -> &Uri {
        &self.head().uri
    }

    /// Read the Request method.
    #[inline]
    pub fn method(&self) -> &Method {
        &self.head().method
    }

    /// Read the Request Version.
    #[inline]
    pub fn version(&self) -> Version {
        self.head().version
    }

    /// The target path of this Request.
    #[inline]
    pub fn path(&self) -> &str {
        self.head().uri.path()
    }

    #[inline]
    /// Returns Request's headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.head().headers
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

    /// Get a reference to the Path parameters.
    ///
    /// Params is a container for url parameters.
    /// A variable segment is specified in the form `{identifier}`,
    /// where the identifier can be used later in a request handler to
    /// access the matched value for that segment.
    #[inline]
    pub fn match_info(&self) -> &Path<Url> {
        &self.path
    }

    /// Request extensions
    #[inline]
    pub fn extensions(&self) -> Ref<Extensions> {
        self.head.extensions()
    }

    /// Mutable reference to a the request's extensions
    #[inline]
    pub fn extensions_mut(&self) -> RefMut<Extensions> {
        self.head.extensions_mut()
    }
}

impl<S> Clone for Request<S> {
    fn clone(&self) -> Request<S> {
        Request {
            head: self.head.clone(),
            path: self.path.clone(),
            state: self.state.clone(),
            payload: self.payload.clone(),
        }
    }
}

impl<S> Deref for Request<S> {
    type Target = RequestHead;

    fn deref(&self) -> &RequestHead {
        self.head()
    }
}

impl<S> HttpMessage for Request<S> {
    type Stream = Payload;

    #[inline]
    fn headers(&self) -> &HeaderMap {
        &self.head.headers
    }

    #[inline]
    fn payload(&self) -> Option<Payload> {
        if let Some(ref pl) = self.payload {
            pl.as_ref().borrow_mut().take()
        } else {
            None
        }
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
            self.head.version,
            self.head.method,
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
