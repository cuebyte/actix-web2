use actix_http::{http::Method, Error, Response};
use actix_service::{IntoNewService, NewService, Service};
use futures::future::{ok, Either, FutureResult};
use futures::{try_ready, Async, Future, IntoFuture, Poll};

use crate::handler::{AsyncFactory, Factory, FromRequest, HandlerRequest};
use crate::responder::Responder;
use crate::route::{CreateRouteService, Route, RouteBuilder, RouteService};
use crate::service::{ServiceRequest, ServiceResponse};

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct Resource<S> {
    routes: Vec<Route<S>>,
}

impl<S: 'static> Resource<S> {
    pub fn build() -> ResourceBuilder<S> {
        ResourceBuilder::new()
    }
}

impl<S: 'static> NewService for Resource<S> {
    type Request = ServiceRequest<S>;
    type Response = ServiceResponse;
    type Error = Error;
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
    type Response = ServiceResponse;
    type Error = Error;
    type Future =
        Either<ResourceServiceResponse, FutureResult<Self::Response, Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        // self.service.poll_ready()
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
        Either::B(ok(req.into()))
    }
}

pub struct ResourceServiceResponse {
    fut: Box<Future<Item = Response, Error = Error>>,
}

impl Future for ResourceServiceResponse {
    type Item = ServiceResponse;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let res = try_ready!(self.fut.poll());
        Ok(Async::Ready(res.into()))
    }
}

pub struct ResourceBuilder<S> {
    routes: Vec<Route<S>>,
}

impl<S: 'static> ResourceBuilder<S> {
    fn new() -> ResourceBuilder<S> {
        ResourceBuilder { routes: Vec::new() }
    }

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
    pub fn route<F>(mut self, f: F) -> ResourceBuilder<S>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::build()));
        self
    }

    /// Register a new `GET` route.
    pub fn get<F>(mut self, f: F) -> ResourceBuilder<S>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new `POST` route.
    pub fn post<F>(mut self, f: F) -> ResourceBuilder<S>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new `PUT` route.
    pub fn put<F>(mut self, f: F) -> ResourceBuilder<S>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new `DELETE` route.
    pub fn delete<F>(mut self, f: F) -> ResourceBuilder<S>
    where
        F: FnOnce(RouteBuilder<S>) -> Route<S>,
    {
        self.routes.push(f(Route::get()));
        self
    }

    /// Register a new `HEAD` route.
    pub fn head<F>(mut self, f: F) -> ResourceBuilder<S>
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
    pub fn method<F>(mut self, method: Method, f: F) -> ResourceBuilder<S>
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
    pub fn to<F, P, R>(mut self, handler: F) -> ResourceBuilder<S>
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
    pub fn to_async<F, P, R>(mut self, handler: F) -> ResourceBuilder<S>
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

    // /// Register a resource middleware
    // ///
    // /// This is similar to `App's` middlewares, but
    // /// middlewares get invoked on resource level.
    // pub fn middleware<M: Middleware<S>>(&mut self, mw: M) {
    //     Rc::get_mut(&mut self.middlewares)
    //         .unwrap()
    //         .push(Box::new(mw));
    // }
}

impl<S: 'static> IntoNewService<Resource<S>> for ResourceBuilder<S> {
    fn into_new_service(self) -> Resource<S> {
        Resource {
            routes: self.routes,
        }
    }
}

// pub struct ResourceServiceBuilder<T, S, U1, U2> {
//     service: T,
//     filters: Vec<Box<Filter<S>>>,
//     _t: PhantomData<(U1, U2)>,
// }

// impl<T, S: 'static, U1, U2> ResourceServiceBuilder<T, S, U1, U2>
// where
//     T: NewService<
//         Request = HandlerRequest<S, U1>,
//         Response = HandlerRequest<S, U2>,
//         Error = Error,
//         InitError = (),
//     >,
// {
//     pub fn new<F: IntoNewService<T>>(factory: F) -> Self {
//         ResourceServiceBuilder {
//             service: factory.into_new_service(),
//             filters: Vec::new(),
//             _t: PhantomData,
//         }
//     }

//     /// Add method match filter to the route.
//     ///
//     /// ```rust
//     /// # extern crate actix_web;
//     /// # use actix_web::*;
//     /// # fn main() {
//     /// App::new().resource("/path", |r| {
//     ///     r.route()
//     ///         .filter(pred::Get())
//     ///         .filter(pred::Header("content-type", "text/plain"))
//     ///         .f(|req| HttpResponse::Ok())
//     /// })
//     /// #      .finish();
//     /// # }
//     /// ```
//     pub fn method(mut self, method: Method) -> Self {
//         self.filters.push(Box::new(filter::Method(method)));
//         self
//     }

//     /// Add filter to the route.
//     ///
//     /// ```rust
//     /// # extern crate actix_web;
//     /// # use actix_web::*;
//     /// # fn main() {
//     /// App::new().resource("/path", |r| {
//     ///     r.route()
//     ///         .filter(pred::Get())
//     ///         .filter(pred::Header("content-type", "text/plain"))
//     ///         .f(|req| HttpResponse::Ok())
//     /// })
//     /// #      .finish();
//     /// # }
//     /// ```
//     pub fn filter<F: Filter<S> + 'static>(&mut self, f: F) -> &mut Self {
//         self.filters.push(Box::new(f));
//         self
//     }

//     pub fn map<T1, U3, F: IntoNewService<T1>>(
//         self,
//         md: F,
//     ) -> ResourceServiceBuilder<
//         impl NewService<
//             Request = HandlerRequest<S, U1>,
//             Response = HandlerRequest<S, U3>,
//             Error = Error,
//             InitError = (),
//         >,
//         S,
//         U1,
//         U2,
//     >
//     where
//         T1: NewService<
//             Request = HandlerRequest<S, U2>,
//             Response = HandlerRequest<S, U3>,
//             InitError = (),
//         >,
//         T1::Error: Into<Error>,
//     {
//         ResourceServiceBuilder {
//             service: self
//                 .service
//                 .and_then(md.into_new_service().map_err(|e| e.into())),
//             filters: self.filters,
//             _t: PhantomData,
//         }
//     }

//     pub fn to_async<F, P, R>(
//         self,
//         handler: F,
//     ) -> Resource<
//         impl NewService<
//             Request = HandlerRequest<S, U1>,
//             Response = Response,
//             Error = Error,
//             InitError = (),
//         >,
//         S,
//     >
//     where
//         F: AsyncFactory<S, U2, P, R>,
//         P: FromRequest<S> + 'static,
//         R: IntoFuture,
//         R::Item: Into<Response>,
//         R::Error: Into<Error>,
//     {
//         Resource {
//             service: self
//                 .service
//                 .and_then(Extract::new(P::Config::default()))
//                 .then(AsyncHandle::new(handler)),
//             filters: Rc::new(self.filters),
//         }
//     }

//     pub fn to<F, P, R>(
//         self,
//         handler: F,
//     ) -> Resource<
//         impl NewService<
//             Request = HandlerRequest<S, U1>,
//             Response = Response,
//             Error = Error,
//             InitError = (),
//         >,
//         S,
//     >
//     where
//         F: Factory<S, U2, P, R> + 'static,
//         P: FromRequest<S> + 'static,
//         R: Responder<S> + 'static,
//     {
//         Resource {
//             service: self
//                 .service
//                 .and_then(Extract::new(P::Config::default()))
//                 .and_then(Handle::new(handler)),
//             filters: Rc::new(self.filters),
//         }
//     }
// }
