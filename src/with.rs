use std::marker::PhantomData;
use std::rc::Rc;

use futures::future::{err, ok, Either, FutureResult};
use futures::{Async, Future, Poll};

use actix_http::{Error, Response};
use actix_net::service::{NewService, Service};

use handler::{AsyncResultItem, FromRequest, Responder};
use request::Request;

#[doc(hidden)]
pub trait WithFactory<S, T, R>: Clone + 'static
where
    R: Responder<S>,
{
    fn call(&self, param: T) -> R;
}

#[doc(hidden)]
pub struct Handle<S, F, T, R>
where
    F: WithFactory<S, T, R>,
    R: Responder<S> + 'static,
{
    hnd: F,
    _t: PhantomData<(S, T, R)>,
}

impl<S, F, T, R> Handle<S, F, T, R>
where
    F: WithFactory<S, T, R>,
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
    F: WithFactory<S, T, R>,
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
    F: WithFactory<S, T, R>,
    R: Responder<S> + 'static,
{
    hnd: F,
    _t: PhantomData<(S, T, R)>,
}

impl<S, F, T, R> Service for HandleService<S, F, T, R>
where
    F: WithFactory<S, T, R>,
    R: Responder<S> + 'static,
{
    type Request = (T, Request<S>);
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
    type Future = Either<FutureResult<(T, Request<S>), Error>, ExtractResponse<T, S>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
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

pub struct ExtractResponse<T, S>
where
    T: FromRequest<S> + 'static,
{
    req: Option<Request<S>>,
    fut: Box<Future<Item = T, Error = Error>>,
}

impl<T, S> Future for ExtractResponse<T, S>
where
    T: FromRequest<S> + 'static,
{
    type Item = (T, Request<S>);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.fut.poll());
        Ok(Async::Ready((item, self.req.take().unwrap())))
    }
}

impl<S, F, R> WithFactory<S, (), R> for F
where
    F: Fn() -> R + Clone + 'static,
    R: Responder<S> + 'static,
{
    fn call(&self, _: ()) -> R {
        (self)()
    }
}

macro_rules! with_factory_tuple ({$(($n:tt, $T:ident)),+} => {
    impl<S, $($T,)+ Func, Res> WithFactory<S, ($($T,)+), Res> for Func
    where Func: Fn($($T,)+) -> Res + Clone + 'static,
        $($T: FromRequest<S> + 'static,)+
          Res: Responder<S> + 'static,
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
