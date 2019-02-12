use std::cell::RefCell;
use std::rc::Rc;

use actix_http::{http::Method, Error, Response};
use actix_service::{
    ApplyNewService, IntoNewService, IntoNewTransform, NewService, NewTransform, Service,
};
use futures::future::{ok, Either, FutureResult};
use futures::{try_ready, Async, Future, IntoFuture, Poll};

use crate::handler::{AsyncFactory, Factory, FromRequest, HandlerRequest};
use crate::responder::Responder;
use crate::route::{CreateRouteService, Route, RouteBuilder, RouteService};
use crate::service::ServiceRequest;

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct Resource<S, T = ResourceEndpoint<S>> {
    routes: Vec<Route<S>>,
    endpoint: T,
    factory_ref: Rc<RefCell<Option<ResourceFactory<S>>>>,
}

impl<S: 'static> Resource<S> {
    pub fn new() -> Resource<S> {
        let fref = Rc::new(RefCell::new(None));

        Resource {
            routes: Vec::new(),
            endpoint: ResourceEndpoint::new(fref.clone()),
            factory_ref: fref,
        }
    }
}

impl<S: 'static, T> Resource<S, T>
where
    T: NewService<
        Request = ServiceRequest<S>,
        Response = Response,
        Error = (),
        InitError = (),
    >,
{
    /// Register a new route and return mutable reference to *Route* object.
    /// *Route* is used for route configuration, i.e. adding predicates,
    /// setting up handler.
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// use actix_web::*;
    ///
    /// fn main() {
    ///     let app = App::new()
    ///         .resource("/", |r| {
    ///             r.route()
    ///                 .filter(pred::Any(pred::Get()).or(pred::Put()))
    ///                 .filter(pred::Header("Content-Type", "text/plain"))
    ///                 .f(|r| HttpResponse::Ok())
    ///         })
    ///         .finish();
    /// }
    /// ```
    pub fn route<F>(mut self, f: F) -> Resource<S, T>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::build()));
        self
    }

    /// Register a new `GET` route.
    pub fn get<F>(mut self, f: F) -> Resource<S, T>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new `POST` route.
    pub fn post<F>(mut self, f: F) -> Resource<S, T>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new `PUT` route.
    pub fn put<F>(mut self, f: F) -> Resource<S, T>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new `DELETE` route.
    pub fn delete<F>(mut self, f: F) -> Resource<S, T>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new `HEAD` route.
    pub fn head<F>(mut self, f: F) -> Resource<S, T>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new route and add method check to route.
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// use actix_web::*;
    /// fn index(req: &HttpRequest) -> HttpResponse { unimplemented!() }
    ///
    /// App::new().resource("/", |r| r.method(http::Method::GET).f(index));
    /// ```
    ///
    /// This is shortcut for:
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// # use actix_web::*;
    /// # fn index(req: &HttpRequest) -> HttpResponse { unimplemented!() }
    /// App::new().resource("/", |r| r.route().filter(pred::Get()).f(index));
    /// ```
    pub fn method<F>(mut self, method: Method, f: F) -> Resource<S, T>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::build().method(method)));
        self
    }

    /// Register a new route and add handler.
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// use actix_web::*;
    /// fn index(req: HttpRequest) -> HttpResponse { unimplemented!() }
    ///
    /// App::new().resource("/", |r| r.with(index));
    /// ```
    ///
    /// This is shortcut for:
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// # use actix_web::*;
    /// # fn index(req: HttpRequest) -> HttpResponse { unimplemented!() }
    /// App::new().resource("/", |r| r.route().with(index));
    /// ```
    pub fn to<F, P, R>(mut self, handler: F) -> Resource<S, T>
    where
        F: Factory<S, (), P, R> + 'static,
        P: FromRequest<S> + 'static,
        R: Responder<S> + 'static,
    {
        self.routes.push(Route::build().to(handler));
        self
    }

    /// Register a new route and add async handler.
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// # extern crate futures;
    /// use actix_web::*;
    /// use futures::future::Future;
    ///
    /// fn index(req: HttpRequest) -> Box<Future<Item=HttpResponse, Error=Error>> {
    ///     unimplemented!()
    /// }
    ///
    /// App::new().resource("/", |r| r.with_async(index));
    /// ```
    ///
    /// This is shortcut for:
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// # extern crate futures;
    /// # use actix_web::*;
    /// # use futures::future::Future;
    /// # fn index(req: HttpRequest) -> Box<Future<Item=HttpResponse, Error=Error>> {
    /// #     unimplemented!()
    /// # }
    /// App::new().resource("/", |r| r.route().with_async(index));
    /// ```
    #[allow(clippy::wrong_self_convention)]
    pub fn to_async<F, P, R>(mut self, handler: F) -> Resource<S, T>
    where
        F: AsyncFactory<S, (), P, R>,
        P: FromRequest<S> + 'static,
        R: IntoFuture + 'static,
        R::Item: Into<Response>,
        R::Error: Into<Error>,
    {
        self.routes.push(Route::build().to_async(handler));
        self
    }

    /// Register a resource middleware
    ///
    /// This is similar to `App's` middlewares, but
    /// middlewares get invoked on resource level.
    pub fn middleware<M, F>(
        self,
        mw: F,
    ) -> Resource<
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
        Resource {
            endpoint,
            routes: self.routes,
            factory_ref: self.factory_ref,
        }
    }
}

impl<S: 'static, T> IntoNewService<T> for Resource<S, T>
where
    T: NewService<
        Request = ServiceRequest<S>,
        Response = Response,
        Error = (),
        InitError = (),
    >,
{
    fn into_new_service(self) -> T {
        *self.factory_ref.borrow_mut() = Some(ResourceFactory {
            routes: self.routes,
        });

        self.endpoint
    }
}

pub struct ResourceFactory<S> {
    routes: Vec<Route<S>>,
}

impl<S: 'static> NewService for ResourceFactory<S> {
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = ResourceService<S>;
    type Future = CreateResourceService<S>;

    fn new_service(&self) -> Self::Future {
        CreateResourceService {
            fut: self
                .routes
                .iter()
                .map(|route| CreateRouteServiceItem::Future(route.new_service()))
                .collect(),
        }
    }
}

enum CreateRouteServiceItem<S> {
    Future(CreateRouteService<S>),
    Service(RouteService<S>),
}

pub struct CreateResourceService<S> {
    fut: Vec<CreateRouteServiceItem<S>>,
}

impl<S: 'static> Future for CreateResourceService<S> {
    type Item = ResourceService<S>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut done = true;

        // poll http services
        for item in &mut self.fut {
            match item {
                CreateRouteServiceItem::Future(ref mut fut) => match fut.poll()? {
                    Async::Ready(route) => {
                        *item = CreateRouteServiceItem::Service(route)
                    }
                    Async::NotReady => {
                        done = false;
                    }
                },
                CreateRouteServiceItem::Service(_) => continue,
            };
        }

        if done {
            let routes = self
                .fut
                .drain(..)
                .map(|item| match item {
                    CreateRouteServiceItem::Service(service) => service,
                    CreateRouteServiceItem::Future(_) => unreachable!(),
                })
                .collect();
            Ok(Async::Ready(ResourceService { routes }))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct ResourceService<S> {
    routes: Vec<RouteService<S>>,
}

impl<S> Service for ResourceService<S> {
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type Future =
        Either<ResourceServiceResponse, FutureResult<Self::Response, Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, mut req: ServiceRequest<S>) -> Self::Future {
        for route in self.routes.iter_mut() {
            if route.check(&mut req) {
                return Either::A(ResourceServiceResponse {
                    fut: route.call(HandlerRequest::new(req.into_request())),
                });
            }
        }
        Either::B(ok(Response::NotFound().finish()))
    }
}

pub struct ResourceServiceResponse {
    fut: Box<Future<Item = Response, Error = Error>>,
}

impl Future for ResourceServiceResponse {
    type Item = Response;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.fut.poll() {
            Ok(Async::Ready(res)) => Ok(Async::Ready(res)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => Ok(Async::Ready(err.into())),
        }
    }
}

#[doc(hidden)]
pub struct ResourceEndpoint<S> {
    factory: Rc<RefCell<Option<ResourceFactory<S>>>>,
}

impl<S> ResourceEndpoint<S> {
    fn new(factory: Rc<RefCell<Option<ResourceFactory<S>>>>) -> Self {
        ResourceEndpoint { factory }
    }
}

impl<S: 'static> NewService for ResourceEndpoint<S> {
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type InitError = ();
    type Service = ResourceEndpointService<S>;
    type Future = ResourceEndpointFactory<S>;

    fn new_service(&self) -> Self::Future {
        ResourceEndpointFactory {
            fut: self.factory.borrow_mut().as_mut().unwrap().new_service(),
        }
    }
}

#[doc(hidden)]
pub struct ResourceEndpointFactory<S> {
    fut: CreateResourceService<S>,
}

impl<S: 'static> Future for ResourceEndpointFactory<S> {
    type Item = ResourceEndpointService<S>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let srv = try_ready!(self.fut.poll());
        Ok(Async::Ready(ResourceEndpointService { srv }))
    }
}

#[doc(hidden)]
pub struct ResourceEndpointService<S: 'static> {
    srv: ResourceService<S>,
}

impl<S: 'static> Service for ResourceEndpointService<S> {
    type Request = ServiceRequest<S>;
    type Response = Response;
    type Error = ();
    type Future =
        Either<ResourceServiceResponse, FutureResult<Self::Response, Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.srv.poll_ready()
    }

    fn call(&mut self, req: ServiceRequest<S>) -> Self::Future {
        self.srv.call(req)
    }
}
