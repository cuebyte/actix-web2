use std::rc::Rc;

use actix_http::{http::HeaderMap, Extensions, HttpMessage, Payload, Request};
use actix_router::{Path, Url};

use crate::request::HttpRequest;

pub struct ServiceRequest<P> {
    req: HttpRequest,
    payload: Payload<P>,
}

impl<P> ServiceRequest<P> {
    pub(crate) fn new(
        path: Path<Url>,
        request: Request<P>,
        extensions: Rc<Extensions>,
    ) -> Self {
        let (head, payload) = request.into_parts();
        ServiceRequest {
            payload,
            req: HttpRequest::new(head, path, extensions),
        }
    }

    #[inline]
    pub fn request(&self) -> &HttpRequest {
        &self.req
    }

    #[inline]
    pub fn into_request(self) -> HttpRequest {
        self.req
    }

    #[inline]
    pub fn match_info_mut(&mut self) -> &mut Path<Url> {
        &mut self.req.path
    }
}

impl<P> HttpMessage for ServiceRequest<P> {
    type Stream = P;

    #[inline]
    fn headers(&self) -> &HeaderMap {
        self.req.headers()
    }

    #[inline]
    fn take_payload(&mut self) -> Payload<Self::Stream> {
        std::mem::replace(&mut self.payload, Payload::None)
    }
}

impl<P> std::ops::Deref for ServiceRequest<P> {
    type Target = HttpRequest;

    fn deref(&self) -> &HttpRequest {
        self.request()
    }
}
