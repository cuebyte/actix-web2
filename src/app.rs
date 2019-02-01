use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;

use actix_http::{Error, Request, Response};
use actix_net::cloneable::CloneableService;
use actix_net::service::{IntoNewService, NewService, Service};
use actix_router::{Path, ResourceInfo, Router, Url};
use futures::future::{ok, FutureResult};
use futures::{Async, Future, Poll};

use crate::handler::{FromRequest, ServiceRequest};
use crate::helpers::{
    not_found, BoxedHttpNewService, BoxedHttpService, DefaultNewService,
    HttpDefaultNewService, HttpDefaultService,
};
use crate::request::Request as WebRequest;

type BoxedResponse = Box<Future<Item = Response, Error = ()>>;

pub trait HttpServiceFactory<S> {
    type Factory: NewService;

    fn path(&self) -> &str;

    fn create(self, state: State<S>) -> Self::Factory;
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
    services: Vec<(String, BoxedHttpNewService<ServiceRequest<S>, Response>)>,
    default: HttpDefaultNewService<ServiceRequest<S>, Response>,
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

impl<S: 'static> App<S> {
    pub fn with(state: S) -> Self {
        App {
            services: Vec::new(),
            default: Box::new(DefaultNewService::new(not_found.into_new_service())),
            state: State::new(state),
        }
    }

    pub fn service<T>(mut self, factory: T) -> Self
    where
        T: HttpServiceFactory<S>,
        T::Factory:
            NewService<Request = ServiceRequest<S>, Response = Response, Error = Error>
                + 'static,
        <T::Factory as NewService>::Future: 'static,
        <T::Factory as NewService>::Service: Service<Request = ServiceRequest<S>>,
        <<T::Factory as NewService>::Service as Service>::Future: 'static,
    {
        let path = factory.path().to_string();
        self.services.push((
            path,
            Box::new(HttpNewService::new(factory.create(self.state.clone()))),
        ));
        self
    }

    pub fn default_service<T, F: IntoNewService<T>>(mut self, factory: F) -> Self
    where
        T: NewService<Request = ServiceRequest<S>, Response = Response> + 'static,
        T::Future: 'static,
        <T::Service as Service>::Future: 'static,
    {
        self.default = Box::new(DefaultNewService::new(factory.into_new_service()));
        self
    }
}

impl<S: 'static> IntoNewService<AppFactory<S>> for App<S> {
    fn into_new_service(self) -> AppFactory<S> {
        AppFactory {
            state: self.state,
            services: Rc::new(self.services),
            default: Rc::new(self.default),
        }
    }
}

#[derive(Clone)]
pub struct AppFactory<S> {
    state: State<S>,
    services: Rc<Vec<(String, BoxedHttpNewService<ServiceRequest<S>, Response>)>>,
    default: Rc<HttpDefaultNewService<ServiceRequest<S>, Response>>,
}

impl<S: 'static> NewService for AppFactory<S> {
    type Request = Request;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = CloneableService<AppService<S>>;
    type Future = CreateService<S>;

    fn new_service(&self) -> Self::Future {
        CreateService {
            fut: self
                .services
                .iter()
                .map(|(path, service)| {
                    CreateServiceItem::Future(Some(path.clone()), service.new_service())
                })
                .collect(),
            state: self.state.clone(),
            default: None,
            default_fut: self.default.new_service(),
        }
    }
}

#[doc(hidden)]
pub struct CreateService<S> {
    fut: Vec<CreateServiceItem<S>>,
    state: State<S>,
    default: Option<HttpDefaultService<ServiceRequest<S>, Response>>,
    default_fut:
        Box<Future<Item = HttpDefaultService<ServiceRequest<S>, Response>, Error = ()>>,
}

enum CreateServiceItem<S> {
    Future(
        Option<String>,
        Box<Future<Item = BoxedHttpService<ServiceRequest<S>, Response>, Error = ()>>,
    ),
    Service(String, BoxedHttpService<ServiceRequest<S>, Response>),
}

impl<S: 'static> Future for CreateService<S> {
    type Item = CloneableService<AppService<S>>;
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
                CreateServiceItem::Future(ref mut path, ref mut fut) => {
                    match fut.poll()? {
                        Async::Ready(service) => Some((path.take().unwrap(), service)),
                        Async::NotReady => {
                            done = false;
                            None
                        }
                    }
                }
                CreateServiceItem::Service(_, _) => continue,
            };

            if let Some((path, service)) = res {
                *item = CreateServiceItem::Service(path, service);
            }
        }

        if done {
            let router = self
                .fut
                .drain(..)
                .fold(Router::build(), |mut router, item| {
                    match item {
                        CreateServiceItem::Service(path, service) => {
                            router.path(&path, service)
                        }
                        CreateServiceItem::Future(_, _) => unreachable!(),
                    }
                    router
                });
            Ok(Async::Ready(CloneableService::new(AppService {
                state: self.state.clone(),
                router: router.finish(),
                default: self.default.take().expect("something is wrong"),
                ready: None,
            })))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct AppService<S> {
    state: State<S>,
    router: Router<BoxedHttpService<ServiceRequest<S>, Response>>,
    default: HttpDefaultService<ServiceRequest<S>, Response>,
    ready: Option<(ServiceRequest<S>, ResourceInfo)>,
}

impl<S> Service for AppService<S> {
    type Request = Request;
    type Response = Response;
    type Error = ();
    type Future = BoxedResponse;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        if self.ready.is_none() {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let mut path = Path::new(Url::new(req.uri().clone()));
        if let Some((srv, _info)) = self.router.recognize_mut(&mut path) {
            srv.call(ServiceRequest::new(WebRequest::new(
                self.state.clone(),
                req,
                path,
            )))
        } else {
            self.default.call(ServiceRequest::new(WebRequest::new(
                self.state.clone(),
                req,
                path,
            )))
        }
    }
}

struct HttpNewService<S, T: NewService<Request = ServiceRequest<S>, Error = Error>>(T);

impl<S, T> HttpNewService<S, T>
where
    T: NewService<Request = ServiceRequest<S>, Response = Response, Error = Error>,
    T::Response: 'static,
    T::Future: 'static,
    <T::Service as Service>::Future: 'static,
{
    pub fn new(service: T) -> Self {
        HttpNewService(service)
    }
}

impl<S, T> NewService for HttpNewService<S, T>
where
    S: 'static,
    T: NewService<Request = ServiceRequest<S>, Response = Response, Error = Error>,
    T::Request: 'static,
    T::Response: 'static,
    T::Future: 'static,
    T::Service: 'static,
    <T::Service as Service>::Future: 'static,
{
    type Request = T::Request;
    type Response = T::Response;
    type Error = ();
    type InitError = ();
    type Service = BoxedHttpService<ServiceRequest<S>, T::Response>;
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

struct HttpServiceWrapper<S, T: Service<Request = ServiceRequest<S>>> {
    service: T,
    _t: PhantomData<(S,)>,
}

impl<S, T> Service for HttpServiceWrapper<S, T>
where
    T::Future: 'static,
    T: Service<Request = ServiceRequest<S>, Response = Response, Error = Error>,
{
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(|_| ())
    }

    fn call(&mut self, req: ServiceRequest<S>) -> Self::Future {
        Box::new(self.service.call(req).then(|res| match res {
            Ok(res) => Ok(res),
            Err(e) => Ok(Response::from(e)),
        }))
    }
}
