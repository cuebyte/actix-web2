use actix_http::dev::ResponseBuilder;
use actix_http::http::StatusCode;
use actix_http::{Error, Response};
use bytes::{Bytes, BytesMut};
use futures::future::{ok, FutureResult};

use handler::Responder;
use request::Request;

impl<S> Responder<S> for ResponseBuilder {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    #[inline]
    fn respond_to(mut self, _: Request<S>) -> Self::Future {
        ok(self.finish())
    }
}

impl<S> Responder<S> for &'static str {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    fn respond_to(self, _: Request<S>) -> Self::Future {
        ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl<S> Responder<S> for &'static [u8] {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    fn respond_to(self, _: Request<S>) -> Self::Future {
        ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}

impl<S> Responder<S> for String {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    fn respond_to(self, _: Request<S>) -> Self::Future {
        ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl<'a, S> Responder<S> for &'a String {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    fn respond_to(self, _: Request<S>) -> Self::Future {
        ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl<S> Responder<S> for Bytes {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    fn respond_to(self, _: Request<S>) -> Self::Future {
        ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}

impl<S> Responder<S> for BytesMut {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    fn respond_to(self, _: Request<S>) -> Self::Future {
        ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}
