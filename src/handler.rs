use futures::future::{err, ok, Either as EitherFuture, Future, FutureResult};
use futures::{Async, Poll};

use actix_http::http::StatusCode;
use actix_http::{Error, Response};

use super::request::Request;

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

/// Trait implemented by types that can be extracted from request.
///
/// Types that implement this trait can be used with `Route::with()` method.
pub trait FromRequest<S>: Sized {
    /// Configuration for conversion process
    type Config: Default;

    /// Future that resolves to a Self
    type Result: Into<AsyncResult<Self>>;

    /// Convert request to a Self
    fn from_request(req: &Request<S>, cfg: &Self::Config) -> Self::Result;

    /// Convert request to a Self
    ///
    /// This method uses default extractor configuration
    fn extract(req: &Request<S>) -> Self::Result {
        Self::from_request(req, &Self::Config::default())
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

/// Convenience trait that converts `Future` object to a `Boxed` future
///
/// For example loading json from request's body is async operation.
///
/// ```rust
/// # extern crate actix_web;
/// # extern crate futures;
/// # #[macro_use] extern crate serde_derive;
/// use actix_web::{
///     App, AsyncResponder, Error, Message, Request, Response,
/// };
/// use futures::future::Future;
///
/// #[derive(Deserialize, Debug)]
/// struct MyObj {
///     name: String,
/// }
///
/// fn index(mut req: Request) -> Box<Future<Item = Response, Error = Error>> {
///     req.json()                   // <- get JsonBody future
///        .from_err()
///        .and_then(|val: MyObj| {  // <- deserialized value
///            Ok(Response::Ok().into())
///        })
///     // Construct boxed future by using `AsyncResponder::responder()` method
///     .responder()
/// }
/// # fn main() {}
/// ```
pub trait AsyncResponder<I, E>: Sized {
    /// Convert to a boxed future
    fn responder(self) -> Box<Future<Item = I, Error = E>>;
}

impl<F, I, E> AsyncResponder<I, E> for F
where
    F: Future<Item = I, Error = E> + 'static,
    I: Responder,
    E: Into<Error>,
{
    fn responder(self) -> Box<Future<Item = I, Error = E>> {
        Box::new(self)
    }
}

/// Represents async result
///
/// Result could be in tree different forms.
/// * Ok(T) - ready item
/// * Err(E) - error happen during reply process
/// * Future<T, E> - reply process completes in the future
pub struct AsyncResult<I, E = Error>(Option<AsyncResultItem<I, E>>);

impl<I, E> Future for AsyncResult<I, E> {
    type Item = I;
    type Error = E;

    fn poll(&mut self) -> Poll<I, E> {
        let res = self.0.take().expect("use after resolve");
        match res {
            AsyncResultItem::Ok(msg) => Ok(Async::Ready(msg)),
            AsyncResultItem::Err(err) => Err(err),
            AsyncResultItem::Future(mut fut) => match fut.poll() {
                Ok(Async::NotReady) => {
                    self.0 = Some(AsyncResultItem::Future(fut));
                    Ok(Async::NotReady)
                }
                Ok(Async::Ready(msg)) => Ok(Async::Ready(msg)),
                Err(err) => Err(err),
            },
        }
    }
}

pub(crate) enum AsyncResultItem<I, E> {
    Ok(I),
    Err(E),
    Future(Box<Future<Item = I, Error = E>>),
}

impl<I, E> AsyncResult<I, E> {
    /// Create async response
    #[inline]
    pub fn async(fut: Box<Future<Item = I, Error = E>>) -> AsyncResult<I, E> {
        AsyncResult(Some(AsyncResultItem::Future(fut)))
    }

    /// Send response
    #[inline]
    pub fn ok<R: Into<I>>(ok: R) -> AsyncResult<I, E> {
        AsyncResult(Some(AsyncResultItem::Ok(ok.into())))
    }

    /// Send error
    #[inline]
    pub fn err<R: Into<E>>(err: R) -> AsyncResult<I, E> {
        AsyncResult(Some(AsyncResultItem::Err(err.into())))
    }

    #[inline]
    pub(crate) fn into(self) -> AsyncResultItem<I, E> {
        self.0.expect("use after resolve")
    }

    #[cfg(test)]
    pub(crate) fn as_msg(&self) -> &I {
        match self.0.as_ref().unwrap() {
            &AsyncResultItem::Ok(ref resp) => resp,
            _ => panic!(),
        }
    }

    #[cfg(test)]
    pub(crate) fn as_err(&self) -> Option<&E> {
        match self.0.as_ref().unwrap() {
            &AsyncResultItem::Err(ref err) => Some(err),
            _ => None,
        }
    }
}

impl<S> Responder<S> for Response {
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    #[inline]
    fn respond_to(self, _: Request<S>) -> Self::Future {
        ok(self)
    }
}

impl<T> From<T> for AsyncResult<T> {
    #[inline]
    fn from(resp: T) -> AsyncResult<T> {
        AsyncResult(Some(AsyncResultItem::Ok(resp)))
    }
}

impl<S, T: Responder<S>, E: Into<Error>> Responder<S> for Result<T, E> {
    type Error = Error;
    type Future = EitherFuture<ResponseFuture<T::Future>, FutureResult<Response, Error>>;

    fn respond_to(self, req: Request<S>) -> Self::Future {
        match self {
            Ok(val) => EitherFuture::A(ResponseFuture::new(val.respond_to(req))),
            Err(e) => EitherFuture::B(err(e.into())),
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

impl<T, E: Into<Error>> From<Result<AsyncResult<T>, E>> for AsyncResult<T> {
    #[inline]
    fn from(res: Result<AsyncResult<T>, E>) -> Self {
        match res {
            Ok(val) => val,
            Err(err) => AsyncResult(Some(AsyncResultItem::Err(err.into()))),
        }
    }
}

impl<T, E: Into<Error>> From<Result<T, E>> for AsyncResult<T> {
    #[inline]
    fn from(res: Result<T, E>) -> Self {
        match res {
            Ok(val) => AsyncResult(Some(AsyncResultItem::Ok(val))),
            Err(err) => AsyncResult(Some(AsyncResultItem::Err(err.into()))),
        }
    }
}

impl<T, E> From<Result<Box<Future<Item = T, Error = E>>, E>> for AsyncResult<T>
where
    T: 'static,
    E: Into<Error> + 'static,
{
    #[inline]
    fn from(res: Result<Box<Future<Item = T, Error = E>>, E>) -> Self {
        match res {
            Ok(fut) => AsyncResult(Some(AsyncResultItem::Future(Box::new(
                fut.map_err(|e| e.into()),
            )))),
            Err(err) => AsyncResult(Some(AsyncResultItem::Err(err.into()))),
        }
    }
}

impl<T> From<Box<Future<Item = T, Error = Error>>> for AsyncResult<T> {
    #[inline]
    fn from(fut: Box<Future<Item = T, Error = Error>>) -> AsyncResult<T> {
        AsyncResult(Some(AsyncResultItem::Future(fut)))
    }
}

/// Convenience type alias
pub type FutureResponse<I, E = Error> = Box<Future<Item = I, Error = E>>;

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
