use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::{Error, Response};
use actix_net::service::{NewService, Service};
use futures::future::{ok, FutureResult};
use futures::{Async, Future, IntoFuture, Poll};

use request::Request;
use responder::{Responder, ResponseFuture};

/// Trait implemented by types that can be extracted from request.
///
/// Types that implement this trait can be used with `Route::with()` method.
pub trait FromRequest<S>: Sized {
    /// Configuration for conversion process
    type Config: Default;

    /// The associated error which can be returned.
    type Error: Into<Error>;

    /// Future that resolves to a Self
    type Future: Future<Item = Self, Error = Self::Error>;

    /// Convert request to a Self
    fn from_request(req: &Request<S>, cfg: &Self::Config) -> Self::Future;

    /// Convert request to a Self
    ///
    /// This method uses default extractor configuration
    fn extract(req: &Request<S>) -> Self::Future {
        Self::from_request(req, &Self::Config::default())
    }
}

/// Handler converter factory
pub trait Factory<S, T, R>: Clone + 'static
where
    R: Responder<S>,
{
    fn call(&self, param: T) -> R;
}

impl<S, F, R> Factory<S, (), R> for F
where
    F: Fn() -> R + Clone + 'static,
    R: Responder<S> + 'static,
{
    fn call(&self, _: ()) -> R {
        (self)()
    }
}

#[doc(hidden)]
pub struct Handle<S, F, T, R>
where
    F: Factory<S, T, R>,
    R: Responder<S> + 'static,
{
    hnd: F,
    _t: PhantomData<(S, T, R)>,
}

impl<S, F, T, R> Handle<S, F, T, R>
where
    F: Factory<S, T, R>,
    R: Responder<S> + 'static,
{
    pub fn new(hnd: F) -> Self {
        Handle {
            hnd,
            _t: PhantomData,
        }
    }
}
impl<S, F, T, R> NewService for Handle<S, F, T, R>
where
    F: Factory<S, T, R>,
    R: Responder<S> + 'static,
{
    type Request = (T, Request<S>);
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = HandleService<S, F, T, R>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(HandleService {
            hnd: self.hnd.clone(),
            _t: PhantomData,
        })
    }
}

#[doc(hidden)]
pub struct HandleService<S, F, T, R>
where
    F: Factory<S, T, R>,
    R: Responder<S> + 'static,
{
    hnd: F,
    _t: PhantomData<(S, T, R)>,
}

impl<S, F, T, R> Service for HandleService<S, F, T, R>
where
    F: Factory<S, T, R>,
    R: Responder<S> + 'static,
{
    type Request = (T, Request<S>);
    type Response = Response;
    type Error = Error;
    type Future = ResponseFuture<R::Future>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, (param, req): Self::Request) -> Self::Future {
        ResponseFuture::new(self.hnd.call(param).respond_to(req))
    }
}

/// Async handler converter factory
pub trait AsyncFactory<S, T, R, I, E>: Clone + 'static
where
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    fn call(&self, param: T) -> R;
}

impl<S, F, R, I, E> AsyncFactory<S, (), R, I, E> for F
where
    F: Fn() -> R + Clone + 'static,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    fn call(&self, _: ()) -> R {
        (self)()
    }
}

#[doc(hidden)]
pub struct AsyncHandle<S, F, T, R, I, E>
where
    F: AsyncFactory<S, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(S, T, R, I, E)>,
}

impl<S, F, T, R, I, E> AsyncHandle<S, F, T, R, I, E>
where
    F: AsyncFactory<S, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    pub fn new(hnd: F) -> Self {
        AsyncHandle {
            hnd,
            _t: PhantomData,
        }
    }
}
impl<S, F, T, R, I, E> NewService for AsyncHandle<S, F, T, R, I, E>
where
    F: AsyncFactory<S, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    type Request = (T, Request<S>);
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = AsyncHandleService<S, F, T, R, I, E>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(AsyncHandleService {
            hnd: self.hnd.clone(),
            _t: PhantomData,
        })
    }
}

#[doc(hidden)]
pub struct AsyncHandleService<S, F, T, R, I, E>
where
    F: AsyncFactory<S, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(S, T, R, I, E)>,
}

impl<S, F, T, R, I, E> Service for AsyncHandleService<S, F, T, R, I, E>
where
    F: AsyncFactory<S, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    type Request = (T, Request<S>);
    type Response = Response;
    type Error = Error;
    type Future = AsyncHandleServiceResponse<R::Future>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, (param, _): Self::Request) -> Self::Future {
        AsyncHandleServiceResponse::new(self.hnd.call(param).into_future())
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
        let res = try_ready!(self.0.poll().map_err(|e| e.into()));
        Ok(Async::Ready(res.into()))
    }
}

pub struct Extract<T, S>
where
    T: FromRequest<S>,
{
    cfg: Rc<T::Config>,
}

impl<T, S> Extract<T, S>
where
    T: FromRequest<S> + 'static,
{
    pub fn new(cfg: T::Config) -> Extract<T, S> {
        Extract { cfg: Rc::new(cfg) }
    }
}
impl<T, S> NewService for Extract<T, S>
where
    T: FromRequest<S> + 'static,
{
    type Request = Request<S>;
    type Response = (T, Request<S>);
    type Error = Error;
    type InitError = ();
    type Service = ExtractService<T, S>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(ExtractService {
            cfg: self.cfg.clone(),
        })
    }
}

pub struct ExtractService<T, S>
where
    T: FromRequest<S>,
{
    cfg: Rc<T::Config>,
}

impl<T, S> Service for ExtractService<T, S>
where
    T: FromRequest<S> + 'static,
{
    type Request = Request<S>;
    type Response = (T, Request<S>);
    type Error = Error;
    type Future = ExtractResponse<T, S>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        ExtractResponse {
            fut: T::from_request(&req, self.cfg.as_ref()),
            req: Some(req),
        }
    }
}

pub struct ExtractResponse<T, S>
where
    T: FromRequest<S> + 'static,
{
    req: Option<Request<S>>,
    fut: T::Future,
}

impl<T, S> Future for ExtractResponse<T, S>
where
    T: FromRequest<S> + 'static,
{
    type Item = (T, Request<S>);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.fut.poll().map_err(|e| e.into()));
        Ok(Async::Ready((item, self.req.take().unwrap())))
    }
}

macro_rules! factory_tuple ({$(($n:tt, $T:ident)),+} => {
    impl<S, $($T,)+ Func, Res> Factory<S, ($($T,)+), Res> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: Responder<S> + 'static,
    {
        fn call(&self, param: ($($T,)+)) -> Res {
            (self)($(param.$n,)+)
        }
    }

    impl<S, $($T,)+ Func, Res, It, Err> AsyncFactory<S, ($($T,)+), Res, It, Err> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: IntoFuture<Item=It, Error=Err> + 'static,
          It: Into<Response>,
          Err: Into<Error>,
    {
        fn call(&self, param: ($($T,)+)) -> Res {
            (self)($(param.$n,)+)
        }
    }

});

factory_tuple!((0, A));
factory_tuple!((0, A), (1, B));
factory_tuple!((0, A), (1, B), (2, C));
factory_tuple!((0, A), (1, B), (2, C), (3, D));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
factory_tuple!(
    (0, A),
    (1, B),
    (2, C),
    (3, D),
    (4, E),
    (5, F),
    (6, G),
    (7, H)
);
factory_tuple!(
    (0, A),
    (1, B),
    (2, C),
    (3, D),
    (4, E),
    (5, F),
    (6, G),
    (7, H),
    (8, I)
);
factory_tuple!(
    (0, A),
    (1, B),
    (2, C),
    (3, D),
    (4, E),
    (5, F),
    (6, G),
    (7, H),
    (8, I),
    (9, J)
);
