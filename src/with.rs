use std::marker::PhantomData;
use std::rc::Rc;

use futures::future::{err, ok, Either, FutureResult};
use futures::{Async, Future, Poll};

use actix_http::{Error, Response};
use actix_net::service::{NewService, Service};

use handler::{AsyncResultItem, FromRequest, Responder};
use request::Request;

#[doc(hidden)]
pub trait WithFactory<T, R>: Clone + 'static
where
    R: Responder,
{
    fn call(&self, param: T) -> R;
}

#[doc(hidden)]
pub struct Handle<F, T, R>
where
    F: WithFactory<T, R>,
    R: Responder + 'static,
{
    hnd: F,
    _t: PhantomData<(T, R)>,
}

impl<F, T, R> Handle<F, T, R>
where
    F: WithFactory<T, R>,
    R: Responder + 'static,
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
    F: WithFactory<T, R>,
    R: Responder + 'static,
{
    type Request = (T, Request);
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
    F: WithFactory<T, R>,
    R: Responder + 'static,
{
    hnd: F,
    _t: PhantomData<(T, R)>,
}

impl<F, T, R> Service for HandleService<F, T, R>
where
    F: WithFactory<T, R>,
    R: Responder + 'static,
{
    type Request = (T, Request);
    type Response = Response;
    type Error = Error;
    type Future = Either<
        FutureResult<Response, Error>,
        Box<Future<Item = Response, Error = Error>>,
    >;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, (param, req): Self::Request) -> Self::Future {
        match self.hnd.call(param).respond_to(req) {
            Ok(res) => match res.into().into() {
                AsyncResultItem::Err(e) => Either::A(err(e)),
                AsyncResultItem::Ok(msg) => Either::A(ok(msg)),
                AsyncResultItem::Future(fut) => Either::B(fut),
            },
            Err(e) => Either::A(err(e.into())),
        }
    }
}

pub struct Extract<T>
where
    T: FromRequest,
{
    cfg: Rc<T::Config>,
}

impl<T> Extract<T>
where
    T: FromRequest + 'static,
{
    pub fn new(cfg: T::Config) -> Extract<T> {
        Extract { cfg: Rc::new(cfg) }
    }
}
impl<T> NewService for Extract<T>
where
    T: FromRequest + 'static,
{
    type Request = Request;
    type Response = (T, Request);
    type Error = Error;
    type InitError = ();
    type Service = ExtractService<T>;
    type Future = FutureResult<Self::Service, ()>;

    fn new_service(&self) -> Self::Future {
        ok(ExtractService {
            cfg: self.cfg.clone(),
        })
    }
}

pub struct ExtractService<T>
where
    T: FromRequest,
{
    cfg: Rc<T::Config>,
}

impl<T> Service for ExtractService<T>
where
    T: FromRequest + 'static,
{
    type Request = Request;
    type Response = (T, Request);
    type Error = Error;
    type Future = Either<FutureResult<(T, Request), Error>, ExtractResponse<T>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let reply = T::from_request(&req, self.cfg.as_ref()).into();
        match reply.into() {
            AsyncResultItem::Err(e) => Either::A(err(e)),
            AsyncResultItem::Ok(msg) => Either::A(ok((msg, req))),
            AsyncResultItem::Future(fut) => Either::B(ExtractResponse {
                fut,
                req: Some(req),
            }),
        }
    }
}

pub struct ExtractResponse<T>
where
    T: FromRequest + 'static,
{
    req: Option<Request>,
    fut: Box<Future<Item = T, Error = Error>>,
}

impl<T> Future for ExtractResponse<T>
where
    T: FromRequest + 'static,
{
    type Item = (T, Request);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.fut.poll());
        Ok(Async::Ready((item, self.req.take().unwrap())))
    }
}

impl<F, R> WithFactory<(), R> for F
where
    F: Fn() -> R + Clone + 'static,
    R: Responder + 'static,
{
    fn call(&self, _: ()) -> R {
        (self)()
    }
}

macro_rules! with_factory_tuple ({$(($n:tt, $T:ident)),+} => {
    impl<$($T,)+ Func, Res> WithFactory<($($T,)+), Res> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest + 'static,)+
          Res: Responder + 'static,
    {
        fn call(&self, param: ($($T,)+)) -> Res {
            (self)($(param.$n,)+)
        }
    }
});

with_factory_tuple!((0, A));
with_factory_tuple!((0, A), (1, B));
with_factory_tuple!((0, A), (1, B), (2, C));
with_factory_tuple!((0, A), (1, B), (2, C), (3, D));
with_factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E));
with_factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
with_factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
with_factory_tuple!(
    (0, A),
    (1, B),
    (2, C),
    (3, D),
    (4, E),
    (5, F),
    (6, G),
    (7, H)
);
with_factory_tuple!(
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
with_factory_tuple!(
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
