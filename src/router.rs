use std::rc::Rc;

use actix_http::{Request, Response};
use actix_net::cloneable::CloneableService;
use actix_net::service::{IntoNewService, NewService, Service};
use futures::future::{ok, FutureResult};
use futures::{Async, Future, Poll};

pub trait HttpService: Service + 'static {
    fn handle(&mut self, req: Request) -> Result<Self::Future, Request>;
}

#[derive(Clone)]
pub struct Router {
    services: Rc<Vec<BoxedHttpNewService>>,
    default: Rc<BoxedNewService>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            services: Rc::new(Vec::new()),
            default: Rc::new(Box::new(DefaultNewService(not_found.into_new_service()))),
        }
    }

    pub fn service<T, F: IntoNewService<T>>(mut self, factory: F) -> Self
    where
        T: NewService<Request = Request, Response = Response> + 'static,
        T::Future: 'static,
        T::Service: HttpService,
        <T::Service as Service>::Future: 'static,
    {
        Rc::get_mut(&mut self.services)
            .expect("multiple copies exist")
            .push(Box::new(HttpNewService(factory.into_new_service())));
        self
    }

    pub fn default_service<T, F: IntoNewService<T>>(mut self, factory: F) -> Self
    where
        T: NewService<Request = Request, Response = Response> + 'static,
        T::Future: 'static,
        <T::Service as Service>::Future: 'static,
    {
        self.default = Rc::new(Box::new(DefaultNewService(factory.into_new_service())));
        self
    }
}

impl NewService for Router {
    type Request = Request;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = CloneableService<RouterService>;
    type Future = RouterFut;

    fn new_service(&self) -> Self::Future {
        RouterFut {
            fut: self
                .services
                .iter()
                .map(|service| RouterFutItem::Future(service.new_service()))
                .collect(),
            default: None,
            default_fut: self.default.new_service(),
        }
    }
}

#[doc(hidden)]
pub struct RouterFut {
    fut: Vec<RouterFutItem>,
    default: Option<BoxedService>,
    default_fut: Box<Future<Item = BoxedService, Error = ()>>,
}

enum RouterFutItem {
    Future(Box<Future<Item = BoxedHttpService, Error = ()>>),
    Service(BoxedHttpService),
}

impl Future for RouterFut {
    type Item = CloneableService<RouterService>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut done = true;

        // poll default handler service
        if self.default.is_none() {
            match self.default_fut.poll()? {
                Async::Ready(service) => self.default = Some(service),
                Async::NotReady => done = false,
            }
        }

        // poll http services
        for item in &mut self.fut {
            let res = match item {
                RouterFutItem::Future(ref mut fut) => match fut.poll()? {
                    Async::Ready(service) => Some(service),
                    Async::NotReady => {
                        done = false;
                        None
                    }
                },
                RouterFutItem::Service(_) => continue,
            };

            if let Some(service) = res {
                *item = RouterFutItem::Service(service);
            }
        }

        if done {
            let services = self
                .fut
                .drain(..)
                .map(|item| match item {
                    RouterFutItem::Service(service) => service,
                    RouterFutItem::Future(_) => unreachable!(),
                }).collect();
            Ok(Async::Ready(CloneableService::new(RouterService {
                services,
                default: self.default.take().expect("something is wrong"),
            })))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct RouterService {
    services: Vec<BoxedHttpService>,
    default: BoxedService,
}

impl Service for RouterService {
    type Request = Request;
    type Response = Response;
    type Error = ();
    type Future = BoxedResponse;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        let mut ready = true;
        for service in &mut self.services {
            if let Async::NotReady = service.poll_ready()? {
                ready = false;
            }
        }
        if ready {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        let mut req = req;
        for item in &mut self.services {
            req = match item.handle(req) {
                Ok(fut) => return fut,
                Err(req) => req,
            };
        }
        self.default.call(req)
    }
}

type BoxedResponse = Box<Future<Item = Response, Error = ()>>;

type BoxedHttpService = Box<
    HttpService<
        Request = Request,
        Response = Response,
        Error = (),
        Future = Box<Future<Item = Response, Error = ()>>,
    >,
>;

type BoxedHttpNewService = Box<
    NewService<
        Request = Request,
        Response = Response,
        Error = (),
        InitError = (),
        Service = BoxedHttpService,
        Future = Box<Future<Item = BoxedHttpService, Error = ()>>,
    >,
>;

struct HttpNewService<T: NewService>(T);

impl<T> NewService for HttpNewService<T>
where
    T: NewService<Request = Request, Response = Response>,
    T::Future: 'static,
    T::Service: HttpService,
    <T::Service as Service>::Future: 'static,
{
    type Request = T::Request;
    type Response = T::Response;
    type Error = ();
    type InitError = ();
    type Service = BoxedHttpService;
    type Future = Box<Future<Item = Self::Service, Error = Self::InitError>>;

    fn new_service(&self) -> Self::Future {
        Box::new(self.0.new_service().map_err(|_| ()).and_then(|service| {
            let service: BoxedHttpService = Box::new(HttpServiceWrapper { service });
            Ok(service)
        }))
    }
}

struct HttpServiceWrapper<T: Service> {
    service: T,
}

impl<T> Service for HttpServiceWrapper<T>
where
    T::Future: 'static,
    T: HttpService,
    T: Service<Request = Request, Response = Response>,
{
    type Request = T::Request;
    type Response = T::Response;
    type Error = ();
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(|_| ())
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        Box::new(self.service.call(req).map_err(|_| ()))
    }
}

impl<T: HttpService> HttpService for HttpServiceWrapper<T>
where
    T::Future: 'static,
    T: Service<Request = Request, Response = Response>,
{
    fn handle(&mut self, req: Request) -> Result<Self::Future, Request> {
        match self.service.handle(req) {
            Ok(fut) => Ok(Box::new(fut.map_err(|_| ()))),
            Err(req) => Err(req),
        }
    }
}

fn not_found(_: Request) -> FutureResult<Response, ()> {
    ok(Response::NotFound().finish())
}

type BoxedService = Box<
    Service<
        Request = Request,
        Response = Response,
        Error = (),
        Future = Box<Future<Item = Response, Error = ()>>,
    >,
>;

type BoxedNewService = Box<
    NewService<
        Request = Request,
        Response = Response,
        Error = (),
        InitError = (),
        Service = BoxedService,
        Future = Box<Future<Item = BoxedService, Error = ()>>,
    >,
>;

struct DefaultNewService<T: NewService>(T);

impl<T> NewService for DefaultNewService<T>
where
    T: NewService<Request = Request, Response = Response> + 'static,
    T::Future: 'static,
    <T::Service as Service>::Future: 'static,
{
    type Request = T::Request;
    type Response = T::Response;
    type Error = ();
    type InitError = ();
    type Service = BoxedService;
    type Future = Box<Future<Item = Self::Service, Error = Self::InitError>>;

    fn new_service(&self) -> Self::Future {
        Box::new(self.0.new_service().map_err(|_| ()).and_then(|service| {
            let service: BoxedService = Box::new(DefaultServiceWrapper { service });
            Ok(service)
        }))
    }
}

struct DefaultServiceWrapper<T: Service> {
    service: T,
}

impl<T> Service for DefaultServiceWrapper<T>
where
    T::Future: 'static,
    T: Service<Request = Request, Response = Response> + 'static,
{
    type Request = T::Request;
    type Response = T::Response;
    type Error = ();
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(|_| ())
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        Box::new(self.service.call(req).map_err(|_| ()))
    }
}
