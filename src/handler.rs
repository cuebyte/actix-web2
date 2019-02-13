use std::marker::PhantomData;

use actix_http::{Error, Response};
use actix_service::{NewService, Service};
use futures::future::{ok, Either, FutureResult};
use futures::{try_ready, Async, Future, IntoFuture, Poll};

use crate::request::HttpRequest;
use crate::responder::{Responder, ResponseFuture};
use crate::service::ServiceRequest;

/// Trait implemented by types that can be extracted from request.
///
/// Types that implement this trait can be used with `Route` handlers.
pub trait FromRequest<P>: Sized {
    /// The associated error which can be returned.
    type Error: Into<Error>;

    /// Future that resolves to a Self
    type Future: Future<Item = Self, Error = Self::Error>;

    /// Convert request to a Self
    fn from_request(req: &mut ServiceRequest<P>) -> Self::Future;
}

/// Handler converter factory
pub trait Factory<T, R>: Clone
where
    R: Responder,
{
    fn call(&self, param: T) -> R;
}

impl<F, R> Factory<(), R> for F
where
    F: Fn() -> R + Clone + 'static,
    R: Responder + 'static,
{
    fn call(&self, _: ()) -> R {
        (self)()
    }
}

#[doc(hidden)]
pub struct Handle<F, T, R>
where
    F: Factory<T, R>,
    R: Responder,
{
    hnd: F,
    _t: PhantomData<(T, R)>,
}

impl<F, T, R> Handle<F, T, R>
where
    F: Factory<T, R>,
    R: Responder,
{
    pub fn new(hnd: F) -> Self {
        Handle {
            hnd,
            _t: PhantomData,
        }
    }
}
impl<F, T, R> NewService for Handle<F, T, R>
where
    F: Factory<T, R>,
    R: Responder + 'static,
{
    type Request = (T, HttpRequest);
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = HandleService<F, T, R>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(HandleService {
            hnd: self.hnd.clone(),
            _t: PhantomData,
        })
    }
}

#[doc(hidden)]
pub struct HandleService<F, T, R>
where
    F: Factory<T, R>,
    R: Responder + 'static,
{
    hnd: F,
    _t: PhantomData<(T, R)>,
}

impl<F, T, R> Service for HandleService<F, T, R>
where
    F: Factory<T, R>,
    R: Responder + 'static,
{
    type Request = (T, HttpRequest);
    type Response = Response;
    type Error = Error;
    type Future = ResponseFuture<R::Future>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, (param, req): (T, HttpRequest)) -> Self::Future {
        ResponseFuture::new(self.hnd.call(param).respond_to(&req))
    }
}

/// Async handler converter factory
pub trait AsyncFactory<T, R>: Clone + 'static
where
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    fn call(&self, param: T) -> R;
}

impl<F, R> AsyncFactory<(), R> for F
where
    F: Fn() -> R + Clone + 'static,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    fn call(&self, _: ()) -> R {
        (self)()
    }
}

#[doc(hidden)]
pub struct AsyncHandle<F, T, R>
where
    F: AsyncFactory<T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(T, R)>,
}

impl<F, T, R> AsyncHandle<F, T, R>
where
    F: AsyncFactory<T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    pub fn new(hnd: F) -> Self {
        AsyncHandle {
            hnd,
            _t: PhantomData,
        }
    }
}
impl<F, T, R> NewService for AsyncHandle<F, T, R>
where
    F: AsyncFactory<T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    type Request = Result<(T, HttpRequest), Error>;
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = AsyncHandleService<F, T, R>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(AsyncHandleService {
            hnd: self.hnd.clone(),
            _t: PhantomData,
        })
    }
}

#[doc(hidden)]
pub struct AsyncHandleService<F, T, R>
where
    F: AsyncFactory<T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(T, R)>,
}

impl<F, T, R> Service for AsyncHandleService<F, T, R>
where
    F: AsyncFactory<T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    type Request = Result<(T, HttpRequest), Error>;
    type Response = Response;
    type Error = Error;
    type Future =
        Either<AsyncHandleServiceResponse<R::Future>, FutureResult<Response, Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Result<(T, HttpRequest), Error>) -> Self::Future {
        match req {
            Ok((param, req)) => Either::A(AsyncHandleServiceResponse::new(
                self.hnd.call(param).into_future(),
            )),
            Err(e) => Either::B(ok(Response::from(e).into_body())),
        }
    }
}

#[doc(hidden)]
pub struct AsyncHandleServiceResponse<T>(T);

impl<T> AsyncHandleServiceResponse<T> {
    pub fn new(fut: T) -> Self {
        AsyncHandleServiceResponse(fut)
    }
}

impl<T> Future for AsyncHandleServiceResponse<T>
where
    T: Future,
    T::Item: Into<Response>,
    T::Error: Into<Error>,
{
    type Item = Response;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(Async::Ready(
            try_ready!(self.0.poll().map_err(|e| e.into())).into(),
        ))
    }
}

/// Extract arguments from request
pub struct Extract<P, T: FromRequest<P>> {
    _t: PhantomData<(P, T)>,
}

impl<P, T: FromRequest<P>> Extract<P, T> {
    pub fn new() -> Self {
        Extract { _t: PhantomData }
    }
}

impl<P, T: FromRequest<P>> Default for Extract<P, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, T: FromRequest<P>> NewService for Extract<P, T> {
    type Request = ServiceRequest<P>;
    type Response = (T, HttpRequest);
    type Error = Error;
    type InitError = ();
    type Service = ExtractService<P, T>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(ExtractService { _t: PhantomData })
    }
}

pub struct ExtractService<P, T: FromRequest<P>> {
    _t: PhantomData<(P, T)>,
}

impl<P, T: FromRequest<P>> Service for ExtractService<P, T> {
    type Request = ServiceRequest<P>;
    type Response = (T, HttpRequest);
    type Error = Error;
    type Future = ExtractResponse<P, T>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, mut req: ServiceRequest<P>) -> Self::Future {
        ExtractResponse {
            fut: T::from_request(&mut req),
            req: Some(req),
        }
    }
}

pub struct ExtractResponse<P, T: FromRequest<P>> {
    req: Option<ServiceRequest<P>>,
    fut: T::Future,
}

impl<P, T: FromRequest<P>> Future for ExtractResponse<P, T> {
    type Item = (T, HttpRequest);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.fut.poll().map_err(|e| e.into()));

        let req = self.req.take().unwrap();
        let req = req.into_request();

        Ok(Async::Ready((item, req)))
    }
}

/// FromRequest trait impl for tuples
macro_rules! factory_tuple ({ $(($n:tt, $T:ident)),+} => {
    impl<Func, $($T,)+ Res> Factory<($($T,)+), Res> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
        //$($T,)+
          Res: Responder + 'static,
    {
        fn call(&self, param: ($($T,)+)) -> Res {
            (self)($(param.$n,)+)
        }
    }

    impl<Func, $($T,)+ Res> AsyncFactory<($($T,)+), Res> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
          Res: IntoFuture + 'static,
          Res::Item: Into<Response>,
          Res::Error: Into<Error>,
    {
        fn call(&self, param: ($($T,)+)) -> Res {
            (self)($(param.$n,)+)
        }
    }
});

#[rustfmt::skip]
mod m {
    use super::*;

factory_tuple!((0, A));
factory_tuple!((0, A), (1, B));
factory_tuple!((0, A), (1, B), (2, C));
factory_tuple!((0, A), (1, B), (2, C), (3, D));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
}
