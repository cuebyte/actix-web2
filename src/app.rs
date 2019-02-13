use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::{Extensions, PayloadStream, Request, Response};
use actix_router::{Path, ResourceDef, ResourceInfo, Router, Url};
use actix_service::{
    AndThenNewService, ApplyNewService, IntoNewService, IntoNewTransform, NewService,
    NewTransform, Service,
};
use futures::future::{ok, Either, FutureResult};
use futures::{try_ready, Async, Future, Poll};

use crate::filter::Filter;
use crate::helpers::{
    BoxedHttpNewService, BoxedHttpService, DefaultNewService, HttpDefaultNewService,
};
use crate::resource::Resource;
use crate::service::ServiceRequest;
use crate::state::{State, StateFactory};

type BoxedResponse = Box<Future<Item = Response, Error = ()>>;

pub trait HttpServiceFactory<Request> {
    type Factory: NewService<Request = Request>;

    fn rdef(&self) -> &ResourceDef;

    fn create(self) -> Self::Factory;
}

/// Application builder
pub struct App<S, T, P = PayloadStream> {
    services: Vec<(
        ResourceDef,
        BoxedHttpNewService<ServiceRequest<P>, Response>,
    )>,
    default: Option<Rc<HttpDefaultNewService<ServiceRequest<P>, Response>>>,
    defaults:
        Vec<Rc<RefCell<Option<Rc<HttpDefaultNewService<ServiceRequest<P>, Response>>>>>>,
    state: AppState<S>,
    filters: Vec<Box<Filter>>,
    endpoint: T,
    factory_ref: Rc<RefCell<Option<AppFactory<P>>>>,
    extensions: Extensions,
    _t: PhantomData<P>,
}

/// Application state
enum AppState<S> {
    St(State<S>),
    Fn(Box<StateFactory<S>>),
}

impl App<(), AppEndpoint<PayloadStream>, PayloadStream> {
    /// Create application with empty state. Application can
    /// be configured with a builder-like pattern.
    pub fn new() -> Self {
        App::create(AppState::St(State::new(())))
    }
}

impl Default for App<(), AppEndpoint<PayloadStream>, PayloadStream> {
    fn default() -> Self {
        App::new()
    }
}

impl<S: 'static> App<S, AppEndpoint<PayloadStream>, PayloadStream> {
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
            defaults: Vec::new(),
            filters: Vec::new(),
            endpoint: AppEndpoint::new(fref.clone()),
            factory_ref: fref,
            extensions: Extensions::new(),
            _t: PhantomData,
        }
    }
}

impl<S: 'static, T, P: 'static> App<S, T, P>
where
    T: NewService<
        Request = ServiceRequest<P>,
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
    pub fn filter<F: Filter + 'static>(mut self, f: F) -> Self {
        self.filters.push(Box::new(f));
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
    pub fn resource<F, U>(mut self, path: &str, f: F) -> Self
    where
        F: FnOnce(Resource<P>) -> Resource<P, U>,
        U: NewService<
                Request = ServiceRequest<P>,
                Response = Response,
                Error = (),
                InitError = (),
            > + 'static,
    {
        let rdef = ResourceDef::new(path);
        let resource = f(Resource::new());
        self.defaults.push(resource.get_default());
        self.services.push((
            rdef,
            Box::new(HttpNewService::new(resource.into_new_service())),
        ));
        self
    }

    /// Register resource handler service.
    pub fn service<R, F, U>(mut self, rdef: R, factory: F) -> Self
    where
        R: Into<ResourceDef>,
        F: IntoNewService<U>,
        U: NewService<Request = ServiceRequest<P>, Response = Response, Error = ()>
            + 'static,
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
            Request = ServiceRequest<P>,
            Response = Response,
            Error = (),
            InitError = (),
        >,
        P,
    >
    where
        M: NewTransform<
            T::Service,
            Request = ServiceRequest<P>,
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
            defaults: Vec::new(),
            filters: self.filters,
            factory_ref: self.factory_ref,
            extensions: Extensions::new(),
            _t: PhantomData,
        }
    }

    /// Default resource to be used if no matching route could be found.
    pub fn default_resource<F, R, U>(mut self, f: F) -> Self
    where
        F: FnOnce(Resource<S>) -> R,
        R: IntoNewService<U>,
        U: NewService<Request = ServiceRequest<P>, Response = Response, Error = ()>
            + 'static,
    {
        // create and configure default resource
        self.default = Some(Rc::new(Box::new(DefaultNewService::new(
            f(Resource::new()).into_new_service(),
        ))));

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

impl<S: 'static, T, P: 'static>
    IntoNewService<AndThenNewService<AppStateFactory<S, P>, T>> for App<S, T, P>
where
    T: NewService<
        Request = ServiceRequest<P>,
        Response = Response,
        Error = (),
        InitError = (),
    >,
{
    fn into_new_service(self) -> AndThenNewService<AppStateFactory<S, P>, T> {
        // update resource default service
        if self.default.is_some() {
            for default in &self.defaults {
                if default.borrow_mut().is_none() {
                    *default.borrow_mut() = self.default.clone();
                }
            }
        }

        // set factory
        *self.factory_ref.borrow_mut() = Some(AppFactory {
            services: Rc::new(self.services),
            filters: Rc::new(self.filters),
        });

        AppStateFactory {
            state: Rc::new(self.state),
            extensions: Rc::new(self.extensions),
            _t: PhantomData,
        }
        .and_then(self.endpoint)
    }
}

/// Service factory to convert `Request` to a `ServiceRequest<S>`
pub struct AppStateFactory<S, P> {
    state: Rc<AppState<S>>,
    extensions: Rc<Extensions>,
    _t: PhantomData<P>,
}

impl<S: 'static, P: 'static> NewService for AppStateFactory<S, P> {
    type Request = Request<P>;
    type Response = ServiceRequest<P>;
    type Error = ();
    type InitError = ();
    type Service = AppStateService<S, P>;
    type Future = Either<
        FutureResult<Self::Service, ()>,
        Box<Future<Item = Self::Service, Error = ()>>,
    >;

    fn new_service(&self) -> Self::Future {
        match self.state.as_ref() {
            AppState::St(ref st) => Either::A(ok(AppStateService {
                state: st.clone(),
                extensions: self.extensions.clone(),
                _t: PhantomData,
            })),
            AppState::Fn(ref f) => {
                let extensions = self.extensions.clone();
                Either::B(Box::new(f.construct().and_then(move |st| {
                    Ok(AppStateService {
                        extensions,
                        state: State::new(st),
                        _t: PhantomData,
                    })
                })))
            }
        }
    }
}

/// Service to convert `Request` to a `ServiceRequest<S>`
pub struct AppStateService<S, P> {
    state: State<S>,
    extensions: Rc<Extensions>,
    _t: PhantomData<P>,
}

impl<S, P> Service for AppStateService<S, P> {
    type Request = Request<P>;
    type Response = ServiceRequest<P>;
    type Error = ();
    type Future = FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Request<P>) -> Self::Future {
        ok(ServiceRequest::new(
            Path::new(Url::new(req.uri().clone())),
            req,
            self.extensions.clone(),
        ))
    }
}

pub struct AppFactory<P> {
    services: Rc<
        Vec<(
            ResourceDef,
            BoxedHttpNewService<ServiceRequest<P>, Response>,
        )>,
    >,
    filters: Rc<Vec<Box<Filter>>>,
}

impl<P> NewService for AppFactory<P> {
    type Request = ServiceRequest<P>;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = AppService<P>;
    type Future = CreateAppService<P>;

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

type HttpServiceFut<P> =
    Box<Future<Item = BoxedHttpService<ServiceRequest<P>, Response>, Error = ()>>;

/// Create app service
#[doc(hidden)]
pub struct CreateAppService<P> {
    fut: Vec<CreateAppServiceItem<P>>,
    filters: Option<Rc<Vec<Box<Filter>>>>,
}

enum CreateAppServiceItem<P> {
    Future(Option<ResourceDef>, HttpServiceFut<P>),
    Service(ResourceDef, BoxedHttpService<ServiceRequest<P>, Response>),
}

impl<P> Future for CreateAppService<P> {
    type Item = AppService<P>;
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
            Ok(Async::Ready(AppService {
                router: router.finish(),
                ready: None,
                filters: self.filters.clone(),
            }))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct AppService<P> {
    router: Router<BoxedHttpService<ServiceRequest<P>, Response>>,
    ready: Option<(ServiceRequest<P>, ResourceInfo)>,
    filters: Option<Rc<Vec<Box<Filter>>>>,
}

impl<P> Service for AppService<P> {
    type Request = ServiceRequest<P>;
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

    fn call(&mut self, mut req: ServiceRequest<P>) -> Self::Future {
        if let Some((srv, _info)) = self.router.recognize_mut(req.match_info_mut()) {
            Either::A(srv.call(req))
        } else {
            Either::B(ok(Response::NotFound().finish()))
        }
    }
}

pub struct AppServiceResponse(Box<Future<Item = Response, Error = ()>>);

impl Future for AppServiceResponse {
    type Item = Response;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll().map_err(|_| panic!())
    }
}

struct HttpNewService<P: 'static, T: NewService<Request = ServiceRequest<P>>>(T);

impl<P, T> HttpNewService<P, T>
where
    T: NewService<Request = ServiceRequest<P>, Response = Response, Error = ()>,
    T::Future: 'static,
    <T::Service as Service>::Future: 'static,
{
    pub fn new(service: T) -> Self {
        HttpNewService(service)
    }
}

impl<P: 'static, T> NewService for HttpNewService<P, T>
where
    T: NewService<Request = ServiceRequest<P>, Response = Response, Error = ()>,
    T::Future: 'static,
    T::Service: 'static,
    <T::Service as Service>::Future: 'static,
{
    type Request = ServiceRequest<P>;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = BoxedHttpService<ServiceRequest<P>, Self::Response>;
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

struct HttpServiceWrapper<T: Service, P> {
    service: T,
    _t: PhantomData<(P,)>,
}

impl<T, P> Service for HttpServiceWrapper<T, P>
where
    T::Future: 'static,
    T: Service<Request = ServiceRequest<P>, Response = Response, Error = ()>,
{
    type Request = ServiceRequest<P>;
    type Response = Response;
    type Error = ();
    type Future = BoxedResponse;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(|_| ())
    }

    fn call(&mut self, req: ServiceRequest<P>) -> Self::Future {
        Box::new(self.service.call(req))
    }
}

#[doc(hidden)]
pub struct AppEndpoint<P> {
    factory: Rc<RefCell<Option<AppFactory<P>>>>,
}

impl<P> AppEndpoint<P> {
    fn new(factory: Rc<RefCell<Option<AppFactory<P>>>>) -> Self {
        AppEndpoint { factory }
    }
}

impl<P> NewService for AppEndpoint<P> {
    type Request = ServiceRequest<P>;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = AppEndpointService<P>;
    type Future = AppEndpointFactory<P>;

    fn new_service(&self) -> Self::Future {
        AppEndpointFactory {
            fut: self.factory.borrow_mut().as_mut().unwrap().new_service(),
        }
    }
}

#[doc(hidden)]
pub struct AppEndpointService<P> {
    app: AppService<P>,
}

impl<P> Service for AppEndpointService<P> {
    type Request = ServiceRequest<P>;
    type Response = Response;
    type Error = ();
    type Future = Either<BoxedResponse, FutureResult<Self::Response, Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.app.poll_ready()
    }

    fn call(&mut self, req: ServiceRequest<P>) -> Self::Future {
        self.app.call(req)
    }
}

#[doc(hidden)]
pub struct AppEndpointFactory<P> {
    fut: CreateAppService<P>,
}

impl<P> Future for AppEndpointFactory<P> {
    type Item = AppEndpointService<P>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let app = try_ready!(self.fut.poll());
        Ok(Async::Ready(AppEndpointService { app }))
    }
}
