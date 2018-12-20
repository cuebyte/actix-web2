use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::h1::Codec;
use actix_http::{Request, Response, SendResponse};
use actix_net::cloneable::CloneableService;
use actix_net::codec::Framed;
use actix_net::service::{IntoNewService, NewService, Service};
use futures::{Async, Future, Poll};
use tokio_io::{AsyncRead, AsyncWrite};

use app::{HttpService, HttpServiceFactory, State};
use helpers::{BoxedHttpNewService, BoxedHttpService, HttpNewService};

pub type FramedRequest<T> = (Request, Framed<T, Codec>);
type BoxedResponse = Box<Future<Item = (), Error = ()>>;

/// Application builder
pub struct FramedApp<T, S = ()> {
    services: Vec<BoxedHttpNewService<FramedRequest<T>, ()>>,
    // default: HttpDefaultNewService<FramedRequest<T>, ()>,
    state: State<S>,
}

impl<T: 'static> FramedApp<T, ()> {
    pub fn new() -> Self {
        FramedApp {
            services: Vec::new(),
            // default: Box::new(DefaultNewService::new(not_found.into_new_service())),
            state: State::new(()),
        }
    }
}

impl<T: 'static, S> FramedApp<T, S> {
    pub fn with(state: S) -> FramedApp<T, S> {
        FramedApp {
            services: Vec::new(),
            // default: Box::new(DefaultNewService::new(not_found.into_new_service())),
            state: State::new(state),
        }
    }

    pub fn service<U>(mut self, factory: U) -> Self
    where
        U: HttpServiceFactory<S>,
        U::Factory: NewService<Request = FramedRequest<T>, Response = ()> + 'static,
        <U::Factory as NewService>::Future: 'static,
        <U::Factory as NewService>::Service: HttpService,
        <<U::Factory as NewService>::Service as Service>::Future: 'static,
    {
        self.services.push(Box::new(HttpNewService::new(
            factory.create(self.state.clone()),
        )));
        self
    }

    pub fn register_service<U>(&mut self, factory: U)
    where
        U: HttpServiceFactory<S>,
        U::Factory: NewService<Request = FramedRequest<T>, Response = ()> + 'static,
        <U::Factory as NewService>::Future: 'static,
        <U::Factory as NewService>::Service: HttpService,
        <<U::Factory as NewService>::Service as Service>::Future: 'static,
    {
        self.services.push(Box::new(HttpNewService::new(
            factory.create(self.state.clone()),
        )));
    }

    // pub fn default_service<U, F: IntoNewService<U>>(mut self, factory: F) -> Self
    // where
    //     U: NewService<Request = FramedRequest<T>, Response = ()> + 'static,
    //     U::Future: 'static,
    //     <U::Service as Service>::Future: 'static,
    // {
    //     self.default = Box::new(DefaultNewService::new(factory.into_new_service()));
    //     self
    // }
}

impl<T: 'static, S> IntoNewService<FramedAppFactory<T>> for FramedApp<T, S>
where
    T: AsyncRead + AsyncWrite,
{
    fn into_new_service(self) -> FramedAppFactory<T> {
        FramedAppFactory {
            services: Rc::new(self.services),
            // default: Rc::new(self.default),
            _t: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct FramedAppFactory<T> {
    services: Rc<Vec<BoxedHttpNewService<FramedRequest<T>, ()>>>,
    // default: Rc<HttpDefaultNewService<FramedRequest<T>, ()>>,
    _t: PhantomData<T>,
}

impl<T: 'static> NewService for FramedAppFactory<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Request = FramedRequest<T>;
    type Response = ();
    type Error = ();
    type InitError = ();
    type Service = CloneableService<FramedAppService<T>>;
    type Future = CreateService<T>;

    fn new_service(&self) -> Self::Future {
        CreateService {
            fut: self
                .services
                .iter()
                .map(|service| CreateServiceItem::Future(service.new_service()))
                .collect(),
            // default: None,
            // default_fut: self.default.new_service(),
        }
    }
}

#[doc(hidden)]
pub struct CreateService<T> {
    fut: Vec<CreateServiceItem<T>>,
    // default: Option<HttpDefaultService<FramedRequest<T>, ()>>,
    // default_fut:
    // Box<Future<Item = HttpDefaultService<FramedRequest<T>, ()>, Error = ()>>,
}

enum CreateServiceItem<T> {
    Future(Box<Future<Item = BoxedHttpService<FramedRequest<T>, ()>, Error = ()>>),
    Service(BoxedHttpService<(Request, Framed<T, Codec>), ()>),
}

impl<T: 'static> Future for CreateService<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Item = CloneableService<FramedAppService<T>>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut done = true;

        // poll default handler service
        // if self.default.is_none() {
        //     match self.default_fut.poll()? {
        //         Async::Ready(service) => self.default = Some(service),
        //         Async::NotReady => done = false,
        //     }
        // }

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
                })
                .collect();
            Ok(Async::Ready(CloneableService::new(FramedAppService {
                services,
                // default: self.default.take().expect("something is wrong"),
                _t: PhantomData,
            })))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct FramedAppService<T> {
    services: Vec<BoxedHttpService<FramedRequest<T>, ()>>,
    // default: HttpDefaultService<FramedRequest<T>, ()>,
    _t: PhantomData<T>,
}

impl<T: 'static> Service for FramedAppService<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Request = FramedRequest<T>;
    type Response = ();
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
        // self.default.call(req)
        Box::new(
            SendResponse::send(req.1, Response::NotFound().finish().into())
                .map(|_| ())
                .map_err(|_| ()),
        )
    }
}
