use std::ops::Deref;
use std::rc::Rc;

use actix_http::{Error, Request, Response};
use actix_net::cloneable::CloneableService;
use actix_net::service::{IntoNewService, NewService, Service};
use futures::future::{ok, FutureResult};
use futures::{Async, Future, Poll};

use handler::FromRequest;
use helpers::{
    not_found, BoxedHttpNewService, BoxedHttpService, DefaultNewService,
    HttpDefaultNewService, HttpDefaultService, HttpNewService,
};
use request::Request as WebRequest;

type BoxedResponse = Box<Future<Item = Response, Error = ()>>;

pub trait HttpServiceFactory<S, Request> {
    type Factory: NewService<Request>;

    fn create(self, state: State<S>) -> Self::Factory;
}

pub trait HttpService<Request>: Service<Request> + 'static {
    fn handle(&mut self, req: Request) -> Result<Self::Future, Request>;
}

/// Application state
pub struct State<S>(Rc<S>);

impl<S> State<S> {
    pub fn new(state: S) -> State<S> {
        State(Rc::new(state))
    }

    pub fn get_ref(&self) -> &S {
        self.0.as_ref()
    }
}

impl<S> Deref for State<S> {
    type Target = S;

    fn deref(&self) -> &S {
        self.0.as_ref()
    }
}

impl<S> Clone for State<S> {
    fn clone(&self) -> State<S> {
        State(self.0.clone())
    }
}

impl<S> FromRequest<S> for State<S> {
    type Config = ();
    type Error = Error;
    type Future = FutureResult<Self, Error>;

    #[inline]
    fn from_request(req: &WebRequest<S>, _: &Self::Config) -> Self::Future {
        ok(req.get_state())
    }
}

/// Application builder
pub struct App<S = ()> {
    services: Vec<BoxedHttpNewService<Request, Response>>,
    default: HttpDefaultNewService<Response>,
    state: State<S>,
}

impl App<()> {
    pub fn new() -> Self {
        App {
            services: Vec::new(),
            default: Box::new(DefaultNewService::new(not_found.into_new_service())),
            state: State::new(()),
        }
    }
}

impl<S> App<S> {
    pub fn with(state: S) -> Self {
        App {
            services: Vec::new(),
            default: Box::new(DefaultNewService::new(not_found.into_new_service())),
            state: State::new(state),
        }
    }

    pub fn service<T>(mut self, factory: T) -> Self
    where
        T: HttpServiceFactory<S, Request>,
        T::Factory: NewService<Request, Response = Response> + 'static,
        <T::Factory as NewService<Request>>::Future: 'static,
        <T::Factory as NewService<Request>>::Service: HttpService<Request>,
        <<T::Factory as NewService<Request>>::Service as Service<Request>>::Future:
            'static,
    {
        self.services.push(Box::new(HttpNewService::new(
            factory.create(self.state.clone()),
        )));
        self
    }

    pub fn default_service<T, F: IntoNewService<T, Request>>(
        mut self,
        factory: F,
    ) -> Self
    where
        T: NewService<Request, Response = Response> + 'static,
        T::Future: 'static,
        <T::Service as Service<Request>>::Future: 'static,
    {
        self.default = Box::new(DefaultNewService::new(factory.into_new_service()));
        self
    }
}

impl<S> IntoNewService<AppFactory, Request> for App<S> {
    fn into_new_service(self) -> AppFactory {
        AppFactory {
            services: Rc::new(self.services),
            default: Rc::new(self.default),
        }
    }
}

#[derive(Clone)]
pub struct AppFactory {
    services: Rc<Vec<BoxedHttpNewService<Request, Response>>>,
    default: Rc<HttpDefaultNewService<Response>>,
}

impl NewService<Request> for AppFactory {
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = CloneableService<AppService>;
    type Future = CreateService;

    fn new_service(&self) -> Self::Future {
        CreateService {
            fut: self
                .services
                .iter()
                .map(|service| CreateServiceItem::Future(service.new_service()))
                .collect(),
            default: None,
            default_fut: self.default.new_service(),
        }
    }
}

#[doc(hidden)]
pub struct CreateService {
    fut: Vec<CreateServiceItem>,
    default: Option<HttpDefaultService<Response>>,
    default_fut: Box<Future<Item = HttpDefaultService<Response>, Error = ()>>,
}

enum CreateServiceItem {
    Future(Box<Future<Item = BoxedHttpService<Request, Response>, Error = ()>>),
    Service(BoxedHttpService<Request, Response>),
}

impl Future for CreateService {
    type Item = CloneableService<AppService>;
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
                CreateServiceItem::Future(ref mut fut) => match fut.poll()? {
                    Async::Ready(service) => Some(service),
                    Async::NotReady => {
                        done = false;
                        None
                    }
                },
                CreateServiceItem::Service(_) => continue,
            };

            if let Some(service) = res {
                *item = CreateServiceItem::Service(service);
            }
        }

        if done {
            let services = self
                .fut
                .drain(..)
                .map(|item| match item {
                    CreateServiceItem::Service(service) => service,
                    CreateServiceItem::Future(_) => unreachable!(),
                }).collect();
            Ok(Async::Ready(CloneableService::new(AppService {
                services,
                default: self.default.take().expect("something is wrong"),
            })))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct AppService {
    services: Vec<BoxedHttpService<Request, Response>>,
    default: HttpDefaultService<Response>,
}

impl Service<Request> for AppService {
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

    fn call(&mut self, req: Request) -> Self::Future {
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
