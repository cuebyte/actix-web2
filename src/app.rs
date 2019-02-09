use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::{Error, Request, Response};
use actix_router::{Path, ResourceInfo, Router, Url};
use actix_service::{IntoNewService, NewService, Service};
use actix_utils::cloneable::CloneableService;
use futures::{Async, Future, Poll};

use crate::handler::ServiceRequest;
use crate::helpers::{
    not_found, BoxedHttpNewService, BoxedHttpService, DefaultNewService,
    HttpDefaultNewService, HttpDefaultService,
};
use crate::request::Request as WebRequest;
use crate::state::{State, StateFactory};

type BoxedResponse = Box<Future<Item = Response, Error = ()>>;

pub trait HttpServiceFactory<Request> {
    type Factory: NewService<Request = Request>;

    fn path(&self) -> &str;

    fn create(self) -> Self::Factory;
}

/// Application builder
pub struct App<S = ()> {
    services: Vec<(String, BoxedHttpNewService<ServiceRequest<S>, Response>)>,
    default: HttpDefaultNewService<ServiceRequest<S>, Response>,
    state: AppState<S>,
}

/// Application state
enum AppState<S> {
    St(State<S>),
    Fn(Box<StateFactory<S>>),
}

impl App<()> {
    /// Create application with empty state. Application can
    /// be configured with a builder-like pattern.
    pub fn new() -> Self {
        App {
            services: Vec::new(),
            default: Box::new(DefaultNewService::new(not_found.into_new_service())),
            state: AppState::St(State::new(())),
        }
    }
}

impl Default for App<()> {
    fn default() -> Self {
        App::new()
    }
}

impl<S: 'static> App<S> {
    /// Create application with specified state. Application can be
    /// configured with a builder-like pattern.
    ///
    /// State is shared with all resources within same application and
    /// could be accessed with `HttpRequest::state()` method.
    ///
    /// **Note**: http server accepts an application factory rather than
    /// an application instance. Http server constructs an application
    /// instance for each thread, thus application state must be constructed
    /// multiple times. If you want to share state between different
    /// threads, a shared object should be used, e.g. `Arc`. Application
    /// state does not need to be `Send` or `Sync`.
    pub fn with_state(state: S) -> Self {
        App {
            services: Vec::new(),
            default: Box::new(DefaultNewService::new(not_found.into_new_service())),
            state: AppState::St(State::new(state)),
        }
    }

    /// Create application with specified state. This function is
    /// similar to `.with_state()` but it accepts state factory. State could
    /// be create asynchronously during application startup.
    pub fn with_state_factory<F>(state: F) -> Self
    where
        F: StateFactory<S> + 'static,
    {
        App {
            services: Vec::new(),
            default: Box::new(DefaultNewService::new(not_found.into_new_service())),
            state: AppState::Fn(Box::new(state)),
        }
    }

    /// Register resource handler service.
    pub fn service<T>(mut self, factory: T) -> Self
    where
        T: HttpServiceFactory<ServiceRequest<S>> + 'static,
        T::Factory:
            NewService<Request = ServiceRequest<S>, Response = Response, Error = Error>
                + 'static,
        <T::Factory as NewService>::Future: 'static,
        <<T::Factory as NewService>::Service as Service>::Future: 'static,
    {
        let path = factory.path().to_string();
        self.services
            .push((path, Box::new(HttpNewService::new(factory.create()))));
        self
    }

    /// Default resource to be used if no matching route could be found.
    pub fn default_service<T, F: IntoNewService<T>>(mut self, factory: F) -> Self
    where
        T: NewService<Request = ServiceRequest<S>, Response = Response> + 'static,
        T::Future: 'static,
        <T::Service as Service>::Future: 'static,
    {
        self.default = Box::new(DefaultNewService::new(factory.into_new_service()));
        self
    }

    /// Register an external resource.
    ///
    /// External resources are useful for URL generation purposes only
    /// and are never considered for matching at request time. Calls to
    /// `HttpRequest::url_for()` will work as expected.
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// use actix_web::{App, HttpRequest, HttpResponse, Result};
    ///
    /// fn index(req: &HttpRequest) -> Result<HttpResponse> {
    ///     let url = req.url_for("youtube", &["oHg5SJYRHA0"])?;
    ///     assert_eq!(url.as_str(), "https://youtube.com/watch/oHg5SJYRHA0");
    ///     Ok(HttpResponse::Ok().into())
    /// }
    ///
    /// fn main() {
    ///     let app = App::new()
    ///         .resource("/index.html", |r| r.get().f(index))
    ///         .external_resource("youtube", "https://youtube.com/watch/{video_id}")
    ///         .finish();
    /// }
    /// ```
    pub fn external_resource<T, U>(self, _name: T, _url: U) -> App<S>
    where
        T: AsRef<str>,
        U: AsRef<str>,
    {
        // self.parts
        //     .as_mut()
        //     .expect("Use after finish")
        //     .router
        //     .register_external(name.as_ref(), ResourceDef::external(url.as_ref()));
        self
    }
}

impl<S: 'static> IntoNewService<AppFactory<S>> for App<S> {
    fn into_new_service(self) -> AppFactory<S> {
        AppFactory {
            state: Rc::new(self.state),
            services: Rc::new(self.services),
            default: Rc::new(self.default),
        }
    }
}

#[derive(Clone)]
pub struct AppFactory<S> {
    state: Rc<AppState<S>>,
    services: Rc<Vec<(String, BoxedHttpNewService<ServiceRequest<S>, Response>)>>,
    default: Rc<HttpDefaultNewService<ServiceRequest<S>, Response>>,
}

impl<S: 'static> NewService for AppFactory<S> {
    type Request = Request;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = CloneableService<AppService<S>>;
    type Future = CreateAppService<S>;

    fn new_service(&self) -> Self::Future {
        let (state, state_fut) = match self.state.as_ref() {
            AppState::St(ref st) => (Some(st.clone()), None),
            AppState::Fn(ref f) => (None, Some(f.construct())),
        };

        CreateAppService {
            fut: self
                .services
                .iter()
                .map(|(path, service)| {
                    CreateAppServiceItem::Future(
                        Some(path.clone()),
                        service.new_service(),
                    )
                })
                .collect(),
            state,
            state_fut,
            default: None,
            default_fut: self.default.new_service(),
        }
    }
}

#[doc(hidden)]
pub struct CreateAppService<S> {
    fut: Vec<CreateAppServiceItem<S>>,
    state: Option<State<S>>,
    state_fut: Option<Box<Future<Item = S, Error = ()>>>,
    default: Option<HttpDefaultService<ServiceRequest<S>, Response>>,
    default_fut:
        Box<Future<Item = HttpDefaultService<ServiceRequest<S>, Response>, Error = ()>>,
}

enum CreateAppServiceItem<S> {
    Future(
        Option<String>,
        Box<Future<Item = BoxedHttpService<ServiceRequest<S>, Response>, Error = ()>>,
    ),
    Service(String, BoxedHttpService<ServiceRequest<S>, Response>),
}

impl<S: 'static> Future for CreateAppService<S> {
    type Item = CloneableService<AppService<S>>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut done = true;

        // poll state factory
        if let Some(ref mut st) = self.state_fut {
            match st.poll()? {
                Async::Ready(state) => self.state = Some(State::new(state)),
                Async::NotReady => done = false,
            }
        }

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
                CreateAppServiceItem::Future(ref mut path, ref mut fut) => {
                    match fut.poll()? {
                        Async::Ready(service) => Some((path.take().unwrap(), service)),
                        Async::NotReady => {
                            done = false;
                            None
                        }
                    }
                }
                CreateAppServiceItem::Service(_, _) => continue,
            };

            if let Some((path, service)) = res {
                *item = CreateAppServiceItem::Service(path, service);
            }
        }

        if done {
            let router = self
                .fut
                .drain(..)
                .fold(Router::build(), |mut router, item| {
                    match item {
                        CreateAppServiceItem::Service(path, service) => {
                            router.path(&path, service)
                        }
                        CreateAppServiceItem::Future(_, _) => unreachable!(),
                    }
                    router
                });
            Ok(Async::Ready(CloneableService::new(AppService {
                state: self.state.take().unwrap(),
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
                path,
                req,
            )))
        } else {
            self.default.call(ServiceRequest::new(WebRequest::new(
                self.state.clone(),
                path,
                req,
            )))
        }
    }
}

struct HttpNewService<S, T: NewService<Request = ServiceRequest<S>, Error = Error>>(
    T,
    PhantomData<(S,)>,
);

impl<S, T> HttpNewService<S, T>
where
    T: NewService<Request = ServiceRequest<S>, Response = Response, Error = Error>,
    T::Response: 'static,
    T::Future: 'static,
    <T::Service as Service>::Future: 'static,
{
    pub fn new(service: T) -> Self {
        HttpNewService(service, PhantomData)
    }
}

impl<S, T> NewService for HttpNewService<S, T>
where
    S: 'static,
    T: NewService<Request = ServiceRequest<S>, Response = Response, Error = Error>,
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

struct HttpServiceWrapper<S, T: Service> {
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
