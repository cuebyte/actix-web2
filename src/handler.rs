use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::{Error, Response};
use actix_service::{NewService, Service};
use futures::future::{ok, Either, FutureResult};
use futures::{try_ready, Async, Future, IntoFuture, Poll};

use crate::request::HttpRequest;
use crate::responder::{Responder, ResponseFuture};

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
    fn from_request(req: &HttpRequest<S>, cfg: &Self::Config) -> Self::Future;

    /// Convert request to a Self
    ///
    /// This method uses default extractor configuration
    fn extract(req: &HttpRequest<S>) -> Self::Future {
        Self::from_request(req, &Self::Config::default())
    }
}

pub struct HandlerRequest<S, Ex = ()> {
    req: HttpRequest<S>,
    param: Ex,
}

impl<S> HandlerRequest<S, ()> {
    pub fn new(req: HttpRequest<S>) -> Self {
        Self { req, param: () }
    }
}

impl<S, Ex> HandlerRequest<S, Ex> {
    pub fn request(&self) -> &HttpRequest<S> {
        &self.req
    }

    pub fn request_mut(&mut self) -> &mut HttpRequest<S> {
        &mut self.req
    }

    pub fn into_parts(self) -> (HttpRequest<S>, Ex) {
        (self.req, self.param)
    }

    pub fn map<Ex2, F>(self, op: F) -> HandlerRequest<S, Ex2>
    where
        F: FnOnce(Ex) -> Ex2,
    {
        HandlerRequest {
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
impl<F, S, Ex, T, R> NewService for Handle<F, S, Ex, T, R>
where
    F: Factory<S, Ex, T, R>,
    R: Responder<S> + 'static,
{
    type Request = (T, HandlerRequest<S, Ex>);
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

impl<F, S, Ex, T, R> Service for HandleService<F, S, Ex, T, R>
where
    F: Factory<S, Ex, T, R>,
    R: Responder<S> + 'static,
{
    type Request = (T, HandlerRequest<S, Ex>);
    type Response = Response;
    type Error = Error;
    type Future = ResponseFuture<R::Future>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, (param, req): (T, HandlerRequest<S, Ex>)) -> Self::Future {
        let (req, ex) = req.into_parts();
        ResponseFuture::new(self.hnd.call(param, ex).respond_to(req))
    }
}

/// Async handler converter factory
pub trait AsyncFactory<S, Ex, T, R>: Clone + 'static
where
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    fn call(&self, param: T, extra: Ex) -> R;
}

impl<F, S, R> AsyncFactory<S, (), (), R> for F
where
    F: Fn() -> R + Clone + 'static,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    fn call(&self, _: (), _: ()) -> R {
        (self)()
    }
}

#[doc(hidden)]
pub struct AsyncHandle<F, S, Ex, T, R>
where
    F: AsyncFactory<S, Ex, T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(S, Ex, T, R)>,
}

impl<F, S, Ex, T, R> AsyncHandle<F, S, Ex, T, R>
where
    F: AsyncFactory<S, Ex, T, R>,
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
impl<F, S, Ex, T, R> NewService for AsyncHandle<F, S, Ex, T, R>
where
    F: AsyncFactory<S, Ex, T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    type Request = Result<(T, HandlerRequest<S, Ex>), Error>;
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = AsyncHandleService<F, S, Ex, T, R>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(AsyncHandleService {
            hnd: self.hnd.clone(),
            _t: PhantomData,
        })
    }
}

#[doc(hidden)]
pub struct AsyncHandleService<F, S, Ex, T, R>
where
    F: AsyncFactory<S, Ex, T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(S, Ex, T, R)>,
}

impl<F, S, Ex, T, R> Service for AsyncHandleService<F, S, Ex, T, R>
where
    F: AsyncFactory<S, Ex, T, R>,
    R: IntoFuture,
    R::Item: Into<Response>,
    R::Error: Into<Error>,
{
    type Request = Result<(T, HandlerRequest<S, Ex>), Error>;
    type Response = Response;
    type Error = Error;
    type Future =
        Either<AsyncHandleServiceResponse<R::Future>, FutureResult<Response, Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Result<(T, HandlerRequest<S, Ex>), Error>) -> Self::Future {
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
        Ok(Async::Ready(
            try_ready!(self.0.poll().map_err(|e| e.into())).into(),
        ))
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
impl<S, T, Ex> NewService for Extract<S, T, Ex>
where
    T: FromRequest<S> + 'static,
{
    type Request = HandlerRequest<S, Ex>;
    type Response = (T, HandlerRequest<S, Ex>);
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

impl<S, T, Ex> Service for ExtractService<S, T, Ex>
where
    T: FromRequest<S> + 'static,
{
    type Request = HandlerRequest<S, Ex>;
    type Response = (T, HandlerRequest<S, Ex>);
    type Error = Error;
    type Future = ExtractResponse<S, T, Ex>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: HandlerRequest<S, Ex>) -> Self::Future {
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
    req: Option<HandlerRequest<S, Ex>>,
    fut: T::Future,
}

impl<S, T, Ex> Future for ExtractResponse<S, T, Ex>
where
    T: FromRequest<S> + 'static,
{
    type Item = (T, HandlerRequest<S, Ex>);
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

    impl<Func, S, $($Ex,)+ $($T,)+ Res> AsyncFactory<S, ($($Ex,)+), ($($T,)+), Res> for Func
    where Func: Fn($($Ex,)+ $($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: IntoFuture + 'static,
          Res::Item: Into<Response>,
          Res::Error: Into<Error>,
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

    impl<Func, S, $($T,)+ Res> AsyncFactory<S, (), ($($T,)+), Res> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: IntoFuture + 'static,
          Res::Item: Into<Response>,
          Res::Error: Into<Error>,
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
