use futures::future::{err, ok, Future};
use futures::{Async, Poll};

use actix_http::http::StatusCode;
use actix_http::{Error, Response};

use super::request::Request;

/// Trait implemented by types that generate responses for clients.
///
/// Types that implement this trait can be used as the return type of a handler.
pub trait Responder<S = ()> {
    /// The associated item which can be returned.
    type Item: Into<AsyncResult<Response>>;

    /// The associated error which can be returned.
    type Error: Into<Error>;

    /// Convert itself to `AsyncResult` or `Error`.
    fn respond_to(self, req: Request<S>) -> Result<Self::Item, Self::Error>;
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
#[derive(Debug)]
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
    type Item = AsyncResult<Response>;
    type Error = Error;

    fn respond_to(self, req: Request<S>) -> Result<AsyncResult<Response>, Error> {
        match self {
            Either::A(a) => match a.respond_to(req) {
                Ok(val) => Ok(val.into()),
                Err(err) => Err(err.into()),
            },
            Either::B(b) => match b.respond_to(req) {
                Ok(val) => Ok(val.into()),
                Err(err) => Err(err.into()),
            },
        }
    }
}

impl<A, B, I, E> Future for Either<A, B>
where
    A: Future<Item = I, Error = E>,
    B: Future<Item = I, Error = E>,
{
    type Item = I;
    type Error = E;

    fn poll(&mut self) -> Poll<I, E> {
        match *self {
            Either::A(ref mut fut) => fut.poll(),
            Either::B(ref mut fut) => fut.poll(),
        }
    }
}

impl<T, S> Responder<S> for Option<T>
where
    T: Responder<S>,
{
    type Item = AsyncResult<Response>;
    type Error = Error;

    fn respond_to(self, req: Request<S>) -> Result<AsyncResult<Response>, Error> {
        match self {
            Some(t) => match t.respond_to(req) {
                Ok(val) => Ok(val.into()),
                Err(err) => Err(err.into()),
            },
            None => Ok(Response::build(StatusCode::NOT_FOUND).finish().into()),
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

impl<S> Responder<S> for AsyncResult<Response> {
    type Item = AsyncResult<Response>;
    type Error = Error;

    fn respond_to(self, _: Request<S>) -> Result<AsyncResult<Response>, Error> {
        Ok(self)
    }
}

impl<S> Responder<S> for Response {
    type Item = AsyncResult<Response>;
    type Error = Error;

    #[inline]
    fn respond_to(self, _: Request<S>) -> Result<AsyncResult<Response>, Error> {
        Ok(AsyncResult(Some(AsyncResultItem::Ok(self))))
    }
}

impl<T> From<T> for AsyncResult<T> {
    #[inline]
    fn from(resp: T) -> AsyncResult<T> {
        AsyncResult(Some(AsyncResultItem::Ok(resp)))
    }
}

impl<S, T: Responder<S>, E: Into<Error>> Responder<S> for Result<T, E> {
    type Item = <T as Responder<S>>::Item;
    type Error = Error;

    fn respond_to(self, req: Request<S>) -> Result<Self::Item, Error> {
        match self {
            Ok(val) => match val.respond_to(req) {
                Ok(val) => Ok(val),
                Err(err) => Err(err.into()),
            },
            Err(err) => Err(err.into()),
        }
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
    type Item = AsyncResult<Response>;
    type Error = Error;

    #[inline]
    fn respond_to(self, req: Request<S>) -> Result<AsyncResult<Response>, Error> {
        let fut = self
            .map_err(|e| e.into())
            .then(move |r| match r.respond_to(req) {
                Ok(reply) => match reply.into().into() {
                    AsyncResultItem::Ok(resp) => ok(resp),
                    _ => panic!("Nested async replies are not supported"),
                },
                Err(e) => err(e),
            });
        Ok(AsyncResult::async(Box::new(fut)))
    }
}
