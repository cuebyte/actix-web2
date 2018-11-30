use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::{Error, Response};
use actix_net::service::{NewService, Service};
use futures::future::{ok, Either, FutureResult};
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

pub struct ServiceRequest<S, Ex = ()> {
    req: Request<S>,
    param: Ex,
}

impl<S> ServiceRequest<S, ()> {
    pub fn new(req: Request<S>) -> Self {
        Self { req, param: () }
    }
}

impl<S, Ex> ServiceRequest<S, Ex> {
    pub fn request(&self) -> &Request<S> {
        &self.req
    }

    pub fn request_mut(&mut self) -> &mut Request<S> {
        &mut self.req
    }

    pub fn into_parts(self) -> (Request<S>, Ex) {
        (self.req, self.param)
    }

    pub fn map<Ex2, F>(self, op: F) -> ServiceRequest<S, Ex2>
    where
        F: FnOnce(Ex) -> Ex2,
    {
        ServiceRequest {
            req: self.req,
            param: op(self.param),
        }
    }
}

/// Handler converter factory
pub trait Factory<S, Ex, T, R>: Clone + 'static
where
    R: Responder<S>,
{
    fn call(&self, param: T, extra: Ex) -> R;
}

impl<S, F, R> Factory<S, (), (), R> for F
where
    F: Fn() -> R + Clone + 'static,
    R: Responder<S> + 'static,
{
    fn call(&self, _: (), _: ()) -> R {
        (self)()
    }
}

#[doc(hidden)]
pub struct Handle<F, S, Ex, T, R>
where
    F: Factory<S, Ex, T, R>,
    R: Responder<S> + 'static,
{
    hnd: F,
    _t: PhantomData<(S, Ex, T, R)>,
}

impl<F, S, Ex, T, R> Handle<F, S, Ex, T, R>
where
    F: Factory<S, Ex, T, R>,
    R: Responder<S> + 'static,
{
    pub fn new(hnd: F) -> Self {
        Handle {
            hnd,
            _t: PhantomData,
        }
    }
}
impl<F, S, Ex, T, R> NewService<(T, ServiceRequest<S, Ex>)> for Handle<F, S, Ex, T, R>
where
    F: Factory<S, Ex, T, R>,
    R: Responder<S> + 'static,
{
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = HandleService<F, S, Ex, T, R>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(HandleService {
            hnd: self.hnd.clone(),
            _t: PhantomData,
        })
    }
}

#[doc(hidden)]
pub struct HandleService<F, S, Ex, T, R>
where
    F: Factory<S, Ex, T, R>,
    R: Responder<S> + 'static,
{
    hnd: F,
    _t: PhantomData<(S, Ex, T, R)>,
}

impl<F, S, Ex, T, R> Service<(T, ServiceRequest<S, Ex>)>
    for HandleService<F, S, Ex, T, R>
where
    F: Factory<S, Ex, T, R>,
    R: Responder<S> + 'static,
{
    type Response = Response;
    type Error = Error;
    type Future = ResponseFuture<R::Future>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, (param, req): (T, ServiceRequest<S, Ex>)) -> Self::Future {
        let (req, ex) = req.into_parts();
        ResponseFuture::new(self.hnd.call(param, ex).respond_to(req))
    }
}

/// Async handler converter factory
pub trait AsyncFactory<S, Ex, T, R, I, E>: Clone + 'static
where
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    fn call(&self, param: T, extra: Ex) -> R;
}

impl<F, S, R, I, E> AsyncFactory<S, (), (), R, I, E> for F
where
    F: Fn() -> R + Clone + 'static,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    fn call(&self, _: (), _: ()) -> R {
        (self)()
    }
}

#[doc(hidden)]
pub struct AsyncHandle<F, S, Ex, T, R, I, E>
where
    F: AsyncFactory<S, Ex, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(S, Ex, T, R, I, E)>,
}

impl<F, S, Ex, T, R, I, E> AsyncHandle<F, S, Ex, T, R, I, E>
where
    F: AsyncFactory<S, Ex, T, R, I, E>,
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
impl<F, S, Ex, T, R, I, E> NewService<Result<(T, ServiceRequest<S, Ex>), Error>>
    for AsyncHandle<F, S, Ex, T, R, I, E>
where
    F: AsyncFactory<S, Ex, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = AsyncHandleService<F, S, Ex, T, R, I, E>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(AsyncHandleService {
            hnd: self.hnd.clone(),
            _t: PhantomData,
        })
    }
}

#[doc(hidden)]
pub struct AsyncHandleService<F, S, Ex, T, R, I, E>
where
    F: AsyncFactory<S, Ex, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(S, Ex, T, R, I, E)>,
}

impl<F, S, Ex, T, R, I, E> Service<Result<(T, ServiceRequest<S, Ex>), Error>>
    for AsyncHandleService<F, S, Ex, T, R, I, E>
where
    F: AsyncFactory<S, Ex, T, R, I, E>,
    R: IntoFuture<Item = I, Error = E>,
    I: Into<Response>,
    E: Into<Error>,
{
    type Response = Response;
    type Error = Error;
    type Future =
        Either<AsyncHandleServiceResponse<R::Future>, FutureResult<Response, Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Result<(T, ServiceRequest<S, Ex>), Error>) -> Self::Future {
        match req {
            Ok((param, req)) => {
                let (_, extra) = req.into_parts();
                Either::A(AsyncHandleServiceResponse::new(
                    self.hnd.call(param, extra).into_future(),
                ))
            }
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
        let res = try_ready!(self.0.poll().map_err(|e| e.into()));
        Ok(Async::Ready(res.into()))
    }
}

pub struct Extract<S, T, Ex>
where
    T: FromRequest<S>,
{
    cfg: Rc<T::Config>,
    _t: PhantomData<Ex>,
}

impl<S, T, Ex> Extract<S, T, Ex>
where
    T: FromRequest<S> + 'static,
{
    pub fn new(cfg: T::Config) -> Extract<S, T, Ex> {
        Extract {
            cfg: Rc::new(cfg),
            _t: PhantomData,
        }
    }
}
impl<S, T, Ex> NewService<ServiceRequest<S, Ex>> for Extract<S, T, Ex>
where
    T: FromRequest<S> + 'static,
{
    type Response = (T, ServiceRequest<S, Ex>);
    type Error = Error;
    type InitError = ();
    type Service = ExtractService<S, T, Ex>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(ExtractService {
            cfg: self.cfg.clone(),
            _t: PhantomData,
        })
    }
}

pub struct ExtractService<S, T, Ex>
where
    T: FromRequest<S>,
{
    cfg: Rc<T::Config>,
    _t: PhantomData<Ex>,
}

impl<S, T, Ex> Service<ServiceRequest<S, Ex>> for ExtractService<S, T, Ex>
where
    T: FromRequest<S> + 'static,
{
    type Response = (T, ServiceRequest<S, Ex>);
    type Error = Error;
    type Future = ExtractResponse<S, T, Ex>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: ServiceRequest<S, Ex>) -> Self::Future {
        ExtractResponse {
            fut: T::from_request(req.request(), self.cfg.as_ref()),
            req: Some(req),
        }
    }
}

pub struct ExtractResponse<S, T, Ex>
where
    T: FromRequest<S> + 'static,
{
    req: Option<ServiceRequest<S, Ex>>,
    fut: T::Future,
}

impl<S, T, Ex> Future for ExtractResponse<S, T, Ex>
where
    T: FromRequest<S> + 'static,
{
    type Item = (T, ServiceRequest<S, Ex>);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.fut.poll().map_err(|e| e.into()));
        Ok(Async::Ready((item, self.req.take().unwrap())))
    }
}

macro_rules! factory_tuple ({ ($(($nex:tt, $Ex:ident)),+), $(($n:tt, $T:ident)),+} => {
    impl<Func, S, $($Ex,)+ $($T,)+ Res> Factory<S, ($($Ex,)+), ($($T,)+), Res> for Func
    where Func: Fn($($Ex,)+ $($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: Responder<S> + 'static,
    {
        fn call(&self, param: ($($T,)+), extra: ($($Ex,)+)) -> Res {
            (self)($(extra.$nex,)+ $(param.$n,)+)
        }
    }

    impl<Func, S, $($Ex,)+ $($T,)+ Res, It, Err> AsyncFactory<S, ($($Ex,)+), ($($T,)+), Res, It, Err> for Func
    where Func: Fn($($Ex,)+ $($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: IntoFuture<Item=It, Error=Err> + 'static,
          It: Into<Response>,
          Err: Into<Error>,
    {
        fn call(&self, param: ($($T,)+), extra: ($($Ex,)+)) -> Res {
            (self)($(extra.$nex,)+ $(param.$n,)+)
        }
    }
});

macro_rules! factory_tuple_unit ({$(($n:tt, $T:ident)),+} => {
    impl<Func, S, $($T,)+ Res> Factory<S, (), ($($T,)+), Res> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: Responder<S> + 'static,
    {
        fn call(&self, param: ($($T,)+), _: ()) -> Res {
            (self)($(param.$n,)+)
        }
    }

    impl<Func, S, $($T,)+ Res, It, Err> AsyncFactory<S, (), ($($T,)+), Res, It, Err> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: IntoFuture<Item=It, Error=Err> + 'static,
          It: Into<Response>,
          Err: Into<Error>,
    {
        fn call(&self, param: ($($T,)+), _: ()) -> Res {
            (self)($(param.$n,)+)
        }
    }
});

#[cfg_attr(rustfmt, rustfmt_skip)]
mod m {
    use super::*;

factory_tuple_unit!((0, A));
factory_tuple!(((0, Aex)), (0, A));
factory_tuple!(((0, Aex), (1, Bex)), (0, A));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A));

factory_tuple_unit!((0, A), (1, B));
factory_tuple!(((0, Aex)), (0, A), (1, B));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B));

factory_tuple_unit!((0, A), (1, B), (2, C));
factory_tuple!(((0, Aex)), (0, A), (1, B), (2, C));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B), (2, C));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B), (2, C));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B), (2, C));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B), (2, C));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B), (2, C));

factory_tuple_unit!((0, A), (1, B), (2, C), (3, D));
factory_tuple!(((0, Aex)), (0, A), (1, B), (2, C), (3, D));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B), (2, C), (3, D));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B), (2, C), (3, D));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B), (2, C), (3, D));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B), (2, C), (3, D));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B), (2, C), (3, D));

factory_tuple_unit!((0, A), (1, B), (2, C), (3, D), (4, E));
factory_tuple!(((0, Aex)), (0, A), (1, B), (2, C), (3, D), (4, E));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B), (2, C), (3, D), (4, E));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B), (2, C), (3, D), (4, E));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B), (2, C), (3, D), (4, E));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B), (2, C), (3, D), (4, E));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B), (2, C), (3, D), (4, E));

factory_tuple_unit!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
factory_tuple!(((0, Aex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F));

factory_tuple_unit!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
factory_tuple!(((0, Aex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));

factory_tuple_unit!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
factory_tuple!(((0, Aex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));

factory_tuple_unit!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
factory_tuple!(((0, Aex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));

factory_tuple_unit!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
factory_tuple!(((0, Aex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
factory_tuple!(((0, Aex), (1, Bex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
factory_tuple!(((0, Aex), (1, Bex), (2, Cex), (3, Dex), (4, Eex), (5, Fex)), (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
}
