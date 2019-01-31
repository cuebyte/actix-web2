use std::marker::PhantomData;

use actix_http::Response;
use actix_service::{NewService, Service};
use futures::future::{ok, FutureResult};
use futures::{Future, Poll};

pub(crate) type BoxedHttpService<Req, Res> = Box<
    Service<
        Req,
        Response = Res,
        Error = (),
        Future = Box<Future<Item = Res, Error = ()>>,
    >,
>;

pub(crate) type BoxedHttpNewService<Req, Res> = Box<
    NewService<
        Req,
        Response = Res,
        Error = (),
        InitError = (),
        Service = BoxedHttpService<Req, Res>,
        Future = Box<Future<Item = BoxedHttpService<Req, Res>, Error = ()>>,
    >,
>;

pub(crate) struct HttpNewService<T: NewService<Request>, Request>(
    T,
    PhantomData<Request>,
);

impl<T, Request> HttpNewService<T, Request>
where
    T: NewService<Request>,
    T::Response: 'static,
    T::Future: 'static,
    T::Service: Service<Request>,
    <T::Service as Service<Request>>::Future: 'static,
{
    pub fn new(service: T) -> Self {
        HttpNewService(service, PhantomData)
    }
}

impl<T, Request> NewService<Request> for HttpNewService<T, Request>
where
    T: NewService<Request>,
    T::Response: 'static,
    T::Future: 'static,
    T::Service: Service<Request> + 'static,
    <T::Service as Service<Request>>::Future: 'static,
    Request: 'static,
{
    type Response = T::Response;
    type Error = ();
    type InitError = ();
    type Service = BoxedHttpService<Request, T::Response>;
    type Future = Box<Future<Item = Self::Service, Error = Self::InitError>>;

    fn new_service(&self) -> Self::Future {
        Box::new(self.0.new_service().map_err(|_| ()).and_then(|service| {
            let service: BoxedHttpService<_, _> = Box::new(HttpServiceWrapper {
                service,
                _t: PhantomData,
            });
            Ok(service)
        }))
    }
}

struct HttpServiceWrapper<T: Service<Request>, Request> {
    service: T,
    _t: PhantomData<Request>,
}

impl<T, Request> Service<Request> for HttpServiceWrapper<T, Request>
where
    T::Response: 'static,
    T::Future: 'static,
    T: Service<Request>,
{
    type Response = T::Response;
    type Error = ();
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(|_| ())
    }

    fn call(&mut self, req: Request) -> Self::Future {
        Box::new(self.service.call(req).map_err(|_| ()))
    }
}

// impl<T: Service<Request>, Request> Service<Request> for HttpServiceWrapper<T, Request>
// where
//     T::Future: 'static,
//     T: Service<Request>,
//     Request: 'static,
// {
//     fn handle(&mut self, req: Request) -> Result<Self::Future, Request> {
//         match self.service.handle(req) {
//             Ok(fut) => Ok(Box::new(fut.map_err(|_| ()))),
//             Err(req) => Err(req),
//         }
//     }
// }

pub(crate) fn not_found<Req>(_: Req) -> FutureResult<Response, ()> {
    ok(Response::NotFound().finish())
}

pub(crate) type HttpDefaultService<Req, Res> = Box<
    Service<
        Req,
        Response = Res,
        Error = (),
        Future = Box<Future<Item = Res, Error = ()>>,
    >,
>;

pub(crate) type HttpDefaultNewService<Req, Res> = Box<
    NewService<
        Req,
        Response = Res,
        Error = (),
        InitError = (),
        Service = HttpDefaultService<Req, Res>,
        Future = Box<Future<Item = HttpDefaultService<Req, Res>, Error = ()>>,
    >,
>;

pub(crate) struct DefaultNewService<T: NewService<Request>, Request> {
    service: T,
    _t: PhantomData<Request>,
}

impl<T, Request> DefaultNewService<T, Request>
where
    T: NewService<Request> + 'static,
    T::Future: 'static,
    <T::Service as Service<Request>>::Future: 'static,
{
    pub fn new(service: T) -> Self {
        DefaultNewService {
            service,
            _t: PhantomData,
        }
    }
}

impl<T, Request> NewService<Request> for DefaultNewService<T, Request>
where
    Request: 'static,
    T: NewService<Request> + 'static,
    T::Future: 'static,
    T::Service: 'static,
    <T::Service as Service<Request>>::Future: 'static,
{
    type Response = T::Response;
    type Error = ();
    type InitError = ();
    type Service = HttpDefaultService<Request, T::Response>;
    type Future = Box<Future<Item = Self::Service, Error = Self::InitError>>;

    fn new_service(&self) -> Self::Future {
        Box::new(
            self.service
                .new_service()
                .map_err(|_| ())
                .and_then(|service| {
                    let service: HttpDefaultService<_, _> =
                        Box::new(DefaultServiceWrapper {
                            service,
                            _t: PhantomData,
                        });
                    Ok(service)
                }),
        )
    }
}

struct DefaultServiceWrapper<T: Service<Request>, Request> {
    service: T,
    _t: PhantomData<Request>,
}

impl<T, Request> Service<Request> for DefaultServiceWrapper<T, Request>
where
    T::Future: 'static,
    T: Service<Request> + 'static,
{
    type Response = T::Response;
    type Error = ();
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(|_| ())
    }

    fn call(&mut self, req: Request) -> Self::Future {
        Box::new(self.service.call(req).map_err(|_| ()))
    }
}
