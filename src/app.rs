use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::{Error, Request, Response};
use actix_router::{Path, ResourceDef, ResourceInfo, Router, Url};
use actix_service::{
    AndThenNewService, ApplyNewService, IntoNewService, IntoNewTransform, NewService,
    NewTransform, Service,
};
use actix_utils::cloneable::CloneableService;
use futures::future::{ok, Either, FutureResult};
use futures::{try_ready, Async, Future, Poll};

use crate::filter::Filter;
use crate::helpers::{BoxedHttpNewService, BoxedHttpService, HttpDefaultNewService};
use crate::request::HttpRequest;
use crate::resource::{Resource, ResourceBuilder};
use crate::service::{ServiceRequest, ServiceResponse};
use crate::state::{State, StateFactory};

type BoxedResponse = Box<Future<Item = Response, Error = ()>>;

pub trait HttpServiceFactory<Request> {
    type Factory: NewService<Request = Request>;

    fn rdef(&self) -> &ResourceDef;

    fn create(self) -> Self::Factory;
}

/// Application builder
pub struct App<S, T> {
    services: Vec<(
        ResourceDef,
        BoxedHttpNewService<ServiceRequest<S>, Response>,
    )>,
    default: Option<HttpDefaultNewService<HttpRequest<S>, Response>>,
    state: AppState<S>,
    filters: Vec<Box<Filter<S>>>,
    endpoint: T,
    factory_ref: Rc<RefCell<Option<AppFactory<S>>>>,
}

/// Application state
enum AppState<S> {
    St(State<S>),
    Fn(Box<StateFactory<S>>),
}

impl App<(), AppEndpoint<()>> {
    /// Create application with empty state. Application can
    /// be configured with a builder-like pattern.
    pub fn new() -> Self {
        App::create(AppState::St(State::new(())))
    }
}

impl Default for App<(), AppEndpoint<()>> {
    fn default() -> Self {
        App::new()
    }
}

impl<S: 'static> App<S, AppEndpoint<S>> {
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
        App::create(AppState::St(State::new(state)))
    }

    /// Create application with specified state. This function is
    /// similar to `.with_state()` but it accepts state factory. State could
    /// be create asynchronously during application startup.
    pub fn with_state_factory<F>(state: F) -> Self
    where
        F: StateFactory<S> + 'static,
    {
        App::create(AppState::Fn(Box::new(state)))
    }

    fn create(state: AppState<S>) -> Self {
        let fref = Rc::new(RefCell::new(None));
        App {
            state,
            services: Vec::new(),
            default: None,
            filters: Vec::new(),
            endpoint: AppEndpoint::new(fref.clone()),
            factory_ref: fref,
        }
    }
}

impl<S: 'static, T> App<S, T>
where
    T: NewService<
        Request = ServiceRequest<S>,
        Response = Response,
        Error = (),
        InitError = (),
    >,
{
    /// Add match predicate to application.
    ///
    /// ```rust
    /// # extern crate actix_web2;
    /// # use actix_web2::*;
    /// # fn main() {
    /// App::new()
    ///     .filter(pred::Host("www.rust-lang.org"))
    ///     .resource("/path", |r| r.f(|_| HttpResponse::Ok()))
    /// #      .finish();
    /// # }
    /// ```
    pub fn filter<P: Filter<S> + 'static>(mut self, p: P) -> Self {
        self.filters.push(Box::new(p));
        self
    }

    /// Configure resource for a specific path.
    ///
    /// Resources may have variable path segments. For example, a
    /// resource with the path `/a/{name}/c` would match all incoming
    /// requests with paths such as `/a/b/c`, `/a/1/c`, or `/a/etc/c`.
    ///
    /// A variable segment is specified in the form `{identifier}`,
    /// where the identifier can be used later in a request handler to
    /// access the matched value for that segment. This is done by
    /// looking up the identifier in the `Params` object returned by
    /// `HttpRequest.match_info()` method.
    ///
    /// By default, each segment matches the regular expression `[^{}/]+`.
    ///
    /// You can also specify a custom regex in the form `{identifier:regex}`:
    ///
    /// For instance, to route `GET`-requests on any route matching
    /// `/users/{userid}/{friend}` and store `userid` and `friend` in
    /// the exposed `Params` object:
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// use actix_web::{http, App, HttpResponse};
    ///
    /// fn main() {
    ///     let app = App::new().resource("/users/{userid}/{friend}", |r| {
    ///         r.get(|r| r.to(|_| HttpResponse::Ok()));
    ///         r.head(|r| r.to(|_| HttpResponse::MethodNotAllowed()))
    ///     });
    /// }
    /// ```
    pub fn resource<F, R, U>(mut self, path: &str, f: F) -> App<S, T>
    where
        F: FnOnce(ResourceBuilder<S>) -> R,
        R: IntoNewService<U>,
        U: NewService<
                Request = ServiceRequest<S>,
                Response = ServiceResponse,
                Error = Error,
            > + 'static,
        U::Future: 'static,
        <U::Service as Service>::Future: 'static,
    {
        let rdef = ResourceDef::new(path);
        self.services.push((
            rdef,
            Box::new(HttpNewService::new(f(Resource::build()).into_new_service())),
        ));
        self
    }

    /// Register resource handler service.
    pub fn service<R, F, U>(mut self, rdef: R, factory: F) -> Self
    where
        R: Into<ResourceDef>,
        F: IntoNewService<U>,
        U: NewService<
                Request = ServiceRequest<S>,
                Response = ServiceResponse,
                Error = Error,
            > + 'static,
        U::Future: 'static,
        <U::Service as Service>::Future: 'static,
    {
        self.services.push((
            rdef.into(),
            Box::new(HttpNewService::new(factory.into_new_service())),
        ));
        self
    }

    /// Register a middleware.
    pub fn middleware<M, F>(
        self,
        mw: F,
    ) -> App<
        S,
        impl NewService<
            Request = ServiceRequest<S>,
            Response = Response,
            Error = (),
            InitError = (),
        >,
    >
    where
        M: NewTransform<
            T::Service,
            Request = ServiceRequest<S>,
            Response = Response,
            Error = (),
            InitError = (),
        >,
        F: IntoNewTransform<M, T::Service>,
    {
        let endpoint = ApplyNewService::new(mw, self.endpoint);
        App {
            endpoint,
            state: self.state,
            services: self.services,
            default: self.default,
            filters: self.filters,
            factory_ref: self.factory_ref,
        }
    }

    // /// Default resource to be used if no matching route could be found.
    // pub fn default_service<U, F: IntoNewService<T>>(mut self, factory: F) -> Self
    // where
    //     U: NewService<Request = HttpRequest<S>, Response = Response> + 'static,
    //     U::Future: 'static,
    //     <U::Service as Service>::Future: 'static,
    // {
    //     self.default =
    //         Some(Box::new(DefaultNewService::new(factory.into_new_service())));
    //     self
    // }

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
    pub fn external_resource<N, U>(self, _name: N, _url: U) -> Self
    where
        N: AsRef<str>,
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

impl<S: 'static, T> IntoNewService<AndThenNewService<AppStateFactory<S>, T>>
    for App<S, T>
where
    T: NewService<
        Request = ServiceRequest<S>,
        Response = Response,
        Error = (),
        InitError = (),
    >,
{
    fn into_new_service(self) -> AndThenNewService<AppStateFactory<S>, T> {
        *self.factory_ref.borrow_mut() = Some(AppFactory {
            services: Rc::new(self.services),
            filters: Rc::new(self.filters),
        });

        AppStateFactory {
            state: Rc::new(self.state),
        }
        .and_then(self.endpoint)
    }
}

pub struct AppStateFactory<S> {
    state: Rc<AppState<S>>,
}

impl<S: 'static> NewService for AppStateFactory<S> {
    type Request = Request;
    type Response = ServiceRequest<S>;
    type Error = ();
    type InitError = ();
    type Service = CloneableService<AppStateService<S>>;
    type Future = Either<
        FutureResult<Self::Service, ()>,
        Box<Future<Item = Self::Service, Error = ()>>,
    >;

    fn new_service(&self) -> Self::Future {
        match self.state.as_ref() {
            AppState::St(ref st) => {
                Either::A(ok(CloneableService::new(AppStateService {
                    state: st.clone(),
                })))
            }
            AppState::Fn(ref f) => Either::B(Box::new(f.construct().and_then(|st| {
                Ok(CloneableService::new(AppStateService {
                    state: State::new(st),
                }))
            }))),
        }
    }
}

pub struct AppStateService<S> {
    state: State<S>,
}

impl<S> Service for AppStateService<S> {
    type Request = Request;
    type Response = ServiceRequest<S>;
    type Error = ();
    type Future = FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        ok(ServiceRequest::new(
            self.state.clone(),
            Path::new(Url::new(req.uri().clone())),
            req,
        ))
    }
}

#[derive(Clone)]
pub struct AppFactory<S> {
    services: Rc<
        Vec<(
            ResourceDef,
            BoxedHttpNewService<ServiceRequest<S>, Response>,
        )>,
    >,
    filters: Rc<Vec<Box<Filter<S>>>>,
}

impl<S: 'static> NewService for AppFactory<S> {
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = CloneableService<AppService<S>>;
    type Future = CreateAppService<S>;

    fn new_service(&self) -> Self::Future {
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
            filters: if self.filters.is_empty() {
                None
            } else {
                Some(self.filters.clone())
            },
        }
    }
}

type HttpServiceFut<S> =
    Box<Future<Item = BoxedHttpService<ServiceRequest<S>, Response>, Error = ()>>;

/// Create app service
#[doc(hidden)]
pub struct CreateAppService<S> {
    fut: Vec<CreateAppServiceItem<S>>,
    filters: Option<Rc<Vec<Box<Filter<S>>>>>,
}

enum CreateAppServiceItem<S> {
    Future(Option<ResourceDef>, HttpServiceFut<S>),
    Service(ResourceDef, BoxedHttpService<ServiceRequest<S>, Response>),
}

impl<S: 'static> Future for CreateAppService<S> {
    type Item = CloneableService<AppService<S>>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut done = true;

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
                            router.rdef(path, service)
                        }
                        CreateAppServiceItem::Future(_, _) => unreachable!(),
                    }
                    router
                });
            Ok(Async::Ready(CloneableService::new(AppService {
                router: router.finish(),
                ready: None,
                filters: self.filters.clone(),
            })))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct AppService<S> {
    router: Router<BoxedHttpService<ServiceRequest<S>, Response>>,
    ready: Option<(ServiceRequest<S>, ResourceInfo)>,
    filters: Option<Rc<Vec<Box<Filter<S>>>>>,
}

impl<S> Service for AppService<S> {
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type Future = Either<BoxedResponse, FutureResult<Self::Response, Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        if self.ready.is_none() {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn call(&mut self, mut req: ServiceRequest<S>) -> Self::Future {
        if let Some((srv, _info)) = self.router.recognize_mut(req.path_mut()) {
            Either::A(srv.call(req))
        } else {
            Either::B(ok(Response::NotFound().finish()))
        }
    }
}

pub struct AppServiceResponse(Box<Future<Item = ServiceResponse, Error = ()>>);

impl Future for AppServiceResponse {
    type Item = ServiceResponse;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll().map_err(|_| panic!())
    }
}

struct HttpNewService<S, T: NewService<Request = ServiceRequest<S>, Error = Error>>(
    T,
    PhantomData<(S,)>,
);

impl<S, T> HttpNewService<S, T>
where
    T: NewService<
        Request = ServiceRequest<S>,
        Response = ServiceResponse,
        Error = Error,
    >,
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
    T: NewService<
        Request = ServiceRequest<S>,
        Response = ServiceResponse,
        Error = Error,
    >,
    T::Future: 'static,
    T::Service: 'static,
    <T::Service as Service>::Future: 'static,
{
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = BoxedHttpService<ServiceRequest<S>, Self::Response>;
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
    S: 'static,
    T::Future: 'static,
    T: Service<Request = ServiceRequest<S>, Response = ServiceResponse, Error = Error>,
{
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type Future = BoxedResponse;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(|_| ())
    }

    fn call(&mut self, req: ServiceRequest<S>) -> Self::Future {
        Box::new(self.service.call(req).then(|res| match res {
            Ok(res) => Ok(res.into()),
            Err(e) => Ok(Response::from(e)),
        }))
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct AppEndpoint<S> {
    factory: Rc<RefCell<Option<AppFactory<S>>>>,
}

impl<S> AppEndpoint<S> {
    fn new(factory: Rc<RefCell<Option<AppFactory<S>>>>) -> Self {
        AppEndpoint { factory }
    }
}

impl<S: 'static> NewService for AppEndpoint<S> {
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = AppEndpointService<S>;
    type Future = AppEndpointFactory<S>;

    fn new_service(&self) -> Self::Future {
        AppEndpointFactory {
            fut: self.factory.borrow_mut().as_mut().unwrap().new_service(),
        }
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct AppEndpointService<S: 'static> {
    app: CloneableService<AppService<S>>,
}

impl<S: 'static> Service for AppEndpointService<S> {
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type Future = Either<BoxedResponse, FutureResult<Self::Response, Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.app.poll_ready()
    }

    fn call(&mut self, req: ServiceRequest<S>) -> Self::Future {
        self.app.call(req)
    }
}

#[doc(hidden)]
pub struct AppEndpointFactory<S: 'static> {
    fut: CreateAppService<S>,
}

impl<S: 'static> Future for AppEndpointFactory<S> {
    type Item = AppEndpointService<S>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let app = try_ready!(self.fut.poll());
        Ok(Async::Ready(AppEndpointService { app }))
    }
}
