use std::ops::{Deref, DerefMut};

use actix_http::Request;
use actix_router::{Path, Url};

use crate::request::HttpRequest;
use crate::state::State;

pub struct ServiceRequest<S> {
    state: State<S>,
    path: Path<Url>,
    request: Request,
}

impl<S> ServiceRequest<S> {
    pub(crate) fn new(state: State<S>, path: Path<Url>, request: Request) -> Self {
        ServiceRequest {
            state,
            path,
            request,
        }
    }

    #[inline]
    /// Shared application state
    pub fn state(&self) -> &S {
        &self.state
    }

    #[inline]
    pub fn into_request(self) -> HttpRequest<S> {
        HttpRequest::new(self.state, self.path, self.request)
    }

    #[inline]
    pub fn path(&self) -> &Path<Url> {
        &self.path
    }

    #[inline]
    pub fn path_mut(&mut self) -> &mut Path<Url> {
        &mut self.path
    }

    // /// Get *ConnectionInfo* for the correct request.
    // #[inline]
    // pub fn connection_info(&self) -> Ref<ConnectionInfo> {
    //     ConnectionInfo::get(self.request.head())
    // }
}

impl<S> Deref for ServiceRequest<S> {
    type Target = Request;

    fn deref(&self) -> &Request {
        &self.request
    }
}

impl<S> DerefMut for ServiceRequest<S> {
    fn deref_mut(&mut self) -> &mut Request {
        &mut self.request
    }
}
