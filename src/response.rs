use actix_http::dev::ResponseBuilder;
use actix_http::http::StatusCode;
use actix_http::{Error, Response};
use bytes::{Bytes, BytesMut};

use handler::Responder;
use request::Request;

impl<S> Responder<S> for ResponseBuilder {
    type Item = Response;
    type Error = Error;

    #[inline]
    fn respond_to(mut self, _: Request<S>) -> Result<Response, Error> {
        Ok(self.finish())
    }
}

impl<S> Responder<S> for &'static str {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request<S>) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl<S> Responder<S> for &'static [u8] {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request<S>) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}

impl<S> Responder<S> for String {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request<S>) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl<'a, S> Responder<S> for &'a String {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request<S>) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl<S> Responder<S> for Bytes {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request<S>) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}

impl<S> Responder<S> for BytesMut {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request<S>) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}
