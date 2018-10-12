use actix_http::dev::ResponseBuilder;
use actix_http::http::StatusCode;
use actix_http::{Error, Response};
use bytes::{Bytes, BytesMut};

use handler::Responder;
use request::Request;

impl Responder for ResponseBuilder {
    type Item = Response;
    type Error = Error;

    #[inline]
    fn respond_to(mut self, _: Request) -> Result<Response, Error> {
        Ok(self.finish())
    }
}

impl Responder for &'static str {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl Responder for &'static [u8] {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}

impl Responder for String {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl<'a> Responder for &'a String {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("text/plain; charset=utf-8")
            .body(self))
    }
}

impl Responder for Bytes {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}

impl Responder for BytesMut {
    type Item = Response;
    type Error = Error;

    fn respond_to(self, _: Request) -> Result<Response, Error> {
        Ok(Response::build(StatusCode::OK)
            .content_type("application/octet-stream")
            .body(self))
    }
}
