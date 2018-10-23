use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::Error;
use actix_net::service::{NewService, Service};
use futures::future::{ok, FutureResult};
use futures::{Async, Future, IntoFuture, Poll};

use handler::FromRequest;
use request::Request;

pub struct FramedRequest<S, T> {
    req: Request<S>,
    framed: T,
}

impl<S, T> FramedRequest<S, T> {
    pub fn new(req: Request<S>, framed: T) -> Self {
        Self { req, framed }
    }
    pub fn request(&self) -> &Request<S> {
        &self.req
    }

    pub fn request_mut(&mut self) -> &mut Request<S> {
        &mut self.req
    }

    pub fn into_parts(self) -> (Request<S>, T) {
        (self.req, self.framed)
    }

    pub fn map<U, F>(self, op: F) -> FramedRequest<S, U>
    where
        F: FnOnce(T) -> U,
    {
        FramedRequest {
            req: self.req,
            framed: op(self.framed),
        }
    }
}

/// T handler converter factory
pub trait FramedFactory<S, T, U, R, E>: Clone + 'static
where
    R: IntoFuture<Item = (), Error = E>,
    E: Into<Error>,
{
    fn call(&self, param: T, framed: U) -> R;
}

#[doc(hidden)]
pub struct FramedHandle<F, S, T, U, R, E>
where
    F: FramedFactory<S, T, U, R, E>,
    R: IntoFuture<Item = (), Error = E>,
    E: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(S, T, U, R, E)>,
}

impl<F, S, T, U, R, E> FramedHandle<F, S, T, U, R, E>
where
    F: FramedFactory<S, T, U, R, E>,
    R: IntoFuture<Item = (), Error = E>,
    E: Into<Error>,
{
    pub fn new(hnd: F) -> Self {
        FramedHandle {
            hnd,
            _t: PhantomData,
        }
    }
}
impl<F, S, T, U, R, E> NewService for FramedHandle<F, S, T, U, R, E>
where
    F: FramedFactory<S, T, U, R, E>,
    R: IntoFuture<Item = (), Error = E>,
    E: Into<Error>,
{
    type Request = (T, FramedRequest<S, U>);
    type Response = ();
    type Error = Error;
    type InitError = ();
    type Service = FramedHandleService<F, S, T, U, R, E>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(FramedHandleService {
            hnd: self.hnd.clone(),
            _t: PhantomData,
        })
    }
}

#[doc(hidden)]
pub struct FramedHandleService<F, S, T, U, R, E>
where
    F: FramedFactory<S, T, U, R, E>,
    R: IntoFuture<Item = (), Error = E>,
    E: Into<Error>,
{
    hnd: F,
    _t: PhantomData<(S, T, U, R, E)>,
}

impl<F, S, T, U, R, E> Service for FramedHandleService<F, S, T, U, R, E>
where
    F: FramedFactory<S, T, U, R, E>,
    R: IntoFuture<Item = (), Error = E>,
    E: Into<Error>,
{
    type Request = (T, FramedRequest<S, U>);
    type Response = ();
    type Error = Error;
    type Future = FramedHandleServiceResponse<R::Future>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, (param, framed): Self::Request) -> Self::Future {
        let (_, framed) = framed.into_parts();
        FramedHandleServiceResponse(self.hnd.call(param, framed).into_future())
    }
}

#[doc(hidden)]
pub struct FramedHandleServiceResponse<T>(T);

impl<T> Future for FramedHandleServiceResponse<T>
where
    T: Future<Item = ()>,
    T::Error: Into<Error>,
{
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let res = try_ready!(self.0.poll().map_err(|e| e.into()));
        Ok(Async::Ready(res.into()))
    }
}

pub struct FramedExtract<S, T, U>
where
    T: FromRequest<S>,
{
    cfg: Rc<T::Config>,
    _t: PhantomData<U>,
}

impl<S, T, U> FramedExtract<S, T, U>
where
    T: FromRequest<S> + 'static,
{
    pub fn new(cfg: T::Config) -> FramedExtract<S, T, U> {
        FramedExtract {
            cfg: Rc::new(cfg),
            _t: PhantomData,
        }
    }
}
impl<S, T, U> NewService for FramedExtract<S, T, U>
where
    T: FromRequest<S> + 'static,
{
    type Request = FramedRequest<S, U>;
    type Response = (T, FramedRequest<S, U>);
    type Error = Error;
    type InitError = ();
    type Service = FramedExtractService<S, T, U>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(FramedExtractService {
            cfg: self.cfg.clone(),
            _t: PhantomData,
        })
    }
}

pub struct FramedExtractService<S, T, U>
where
    T: FromRequest<S>,
{
    cfg: Rc<T::Config>,
    _t: PhantomData<U>,
}

impl<S, T, U> Service for FramedExtractService<S, T, U>
where
    T: FromRequest<S> + 'static,
{
    type Request = FramedRequest<S, U>;
    type Response = (T, FramedRequest<S, U>);
    type Error = Error;
    type Future = FramedExtractResponse<S, T, U>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        FramedExtractResponse {
            fut: T::from_request(&req.request(), self.cfg.as_ref()),
            req: Some(req),
        }
    }
}

pub struct FramedExtractResponse<S, T, U>
where
    T: FromRequest<S> + 'static,
{
    req: Option<FramedRequest<S, U>>,
    fut: T::Future,
}

impl<S, T, U> Future for FramedExtractResponse<S, T, U>
where
    T: FromRequest<S> + 'static,
{
    type Item = (T, FramedRequest<S, U>);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.fut.poll().map_err(|e| e.into()));
        Ok(Async::Ready((item, self.req.take().unwrap())))
    }
}

macro_rules! factory_tuple ({$(($n:tt, $T:ident)),+} => {
    impl<Func, S, $($T,)+ Frm, Res, Err> FramedFactory<S, ($($T,)+), Frm, Res, Err> for Func
    where Func: Fn(Frm, $($T,)+) -> Res + Clone + 'static,
         $($T: FromRequest<S> + 'static,)+
          Res: IntoFuture<Item=(), Error=Err> + 'static,
          Err: Into<Error>,
    {
        fn call(&self, param: ($($T,)+), framed: Frm) -> Res {
            (self)(framed, $(param.$n,)+)
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
