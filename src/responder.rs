use actix_http::dev::ResponseBuilder;
use actix_http::http::StatusCode;
use actix_http::{Error, Response};
use bytes::{Bytes, BytesMut};
use futures::future::{err, ok, Either as EitherFuture, FutureResult};
use futures::{Future, Poll};

use request::Request;

/// Trait implemented by types that generate responses for clients.
///
/// Types that implement this trait can be used as the return type of a handler.
pub trait Responder<S = ()> {
    /// The associated error which can be returned.
    type Error: Into<Error>;

    /// The future response value.
    type Future: Future<Item = Response, Error = Self::Error>;

    /// Convert itself to `AsyncResult` or `Error`.
    fn respond_to(self, req: Request<S>) -> Self::Future;
}

impl<S> Responder<S> for Response {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    #[inline]
    fn respond_to(self, _: Request<S>) -> Self::Future {
        ok(self)
    }
}

impl<T, S> Responder<S> for Option<T>
where
    T: Responder<S>,
{
    type Error = T::Error;
    type Future = EitherFuture<T::Future, FutureResult<Response, T::Error>>;

    fn respond_to(self, req: Request<S>) -> Self::Future {
        match self {
            Some(t) => EitherFuture::A(t.respond_to(req)),
            None => EitherFuture::B(ok(Response::build(StatusCode::NOT_FOUND)
                .finish()
                .into())),
        }
    }
}

impl<S, T, E> Responder<S> for Result<T, E>
where
    T: Responder<S>,
    E: Into<Error>,
{
    type Error = Error;
    type Future = EitherFuture<ResponseFuture<T::Future>, FutureResult<Response, Error>>;

    fn respond_to(self, req: Request<S>) -> Self::Future {
        match self {
            Ok(val) => EitherFuture::A(ResponseFuture::new(val.respond_to(req))),
            Err(e) => EitherFuture::B(err(e.into())),
        }
    }
}

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

/// Combines two different responder types into a single type
///
/// ```rust,ignore
/// # extern crate actix_web;
/// # extern crate futures;
/// # use futures::future::Future;
/// use actix_web::{AsyncResponder, Either, Error, Request, Response};
/// use futures::future::result;
///
/// type RegisterResult =
///     Either<Response, Box<Future<Item = Response, Error = Error>>>;
///
/// fn index(req: Request) -> RegisterResult {
///     if is_a_variant() {
///         // <- choose variant A
///         Either::A(Response::BadRequest().body("Bad data"))
///     } else {
///         Either::B(
///             // <- variant B
///             result(Ok(Response::Ok()
///                 .content_type("text/html")
///                 .body("Hello!")))
///                 .responder(),
///         )
///     }
/// }
/// # fn is_a_variant() -> bool { true }
/// # fn main() {}
/// ```
pub enum Either<A, B> {
    /// First branch of the type
    A(A),
    /// Second branch of the type
    B(B),
}

impl<A, B, S> Responder<S> for Either<A, B>
where
    A: Responder<S>,
    B: Responder<S>,
{
    type Error = Error;
    type Future = EitherResponder<A::Future, B::Future>;

    fn respond_to(self, req: Request<S>) -> Self::Future {
        match self {
            Either::A(a) => EitherResponder::A(a.respond_to(req)),
            Either::B(b) => EitherResponder::B(b.respond_to(req)),
        }
    }
}

pub enum EitherResponder<A, B>
where
    A: Future<Item = Response>,
    A::Error: Into<Error>,
    B: Future<Item = Response>,
    B::Error: Into<Error>,
{
    A(A),
    B(B),
}

impl<A, B> Future for EitherResponder<A, B>
where
    A: Future<Item = Response>,
    A::Error: Into<Error>,
    B: Future<Item = Response>,
    B::Error: Into<Error>,
{
    type Item = Response;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self {
            EitherResponder::A(ref mut fut) => Ok(fut.poll().map_err(|e| e.into())?),
            EitherResponder::B(ref mut fut) => Ok(fut.poll().map_err(|e| e.into())?),
        }
    }
}

pub struct ResponseFuture<T>(T);

impl<T> ResponseFuture<T> {
    pub fn new(fut: T) -> Self {
        ResponseFuture(fut)
    }
}

impl<T> Future for ResponseFuture<T>
where
    T: Future<Item = Response>,
    T::Error: Into<Error>,
{
    type Item = Response;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(self.0.poll().map_err(|e| e.into())?)
    }
}

impl<I, E, S> Responder<S> for Box<Future<Item = I, Error = E>>
where
    S: 'static,
    I: Responder<S> + 'static,
    E: Into<Error> + 'static,
{
    type Error = Error;
    type Future = Box<Future<Item = Response, Error = Error>>;

    #[inline]
    fn respond_to(self, req: Request<S>) -> Self::Future {
        Box::new(
            self.map_err(|e| e.into())
                .and_then(move |r| ResponseFuture(r.respond_to(req))),
        )
    }
}
