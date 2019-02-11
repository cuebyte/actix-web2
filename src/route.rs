use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::{http::Method, Error, Response};
use actix_service::{NewService, Service};
use futures::{Async, Future, IntoFuture, Poll};

use crate::filter::{self, Filter};
use crate::handler::{
    AsyncFactory, AsyncHandle, Extract, Factory, FromRequest, Handle, HandlerRequest,
};
use crate::responder::Responder;
use crate::service::ServiceRequest;

pub(crate) type BoxedRouteService<Req, Res> = Box<
    Service<
        Request = Req,
        Response = Res,
        Error = Error,
        Future = Box<Future<Item = Res, Error = Error>>,
    >,
>;

pub(crate) type BoxedRouteNewService<Req, Res> = Box<
    NewService<
        Request = Req,
        Response = Res,
        Error = Error,
        InitError = (),
        Service = BoxedRouteService<Req, Res>,
        Future = Box<Future<Item = BoxedRouteService<Req, Res>, Error = ()>>,
    >,
>;

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct Route<S> {
    service: BoxedRouteNewService<HandlerRequest<S>, Response>,
    filters: Rc<Vec<Box<Filter<S>>>>,
}

impl<S: 'static> Route<S> {
    pub fn build() -> RouteBuilder<S> {
        RouteBuilder::new()
    }

    pub fn get() -> RouteBuilder<S> {
        RouteBuilder::new().method(Method::GET)
    }

    pub fn post() -> RouteBuilder<S> {
        RouteBuilder::new().method(Method::POST)
    }

    pub fn put() -> RouteBuilder<S> {
        RouteBuilder::new().method(Method::PUT)
    }

    pub fn delete() -> RouteBuilder<S> {
        RouteBuilder::new().method(Method::DELETE)
    }
}

impl<S: 'static> NewService for Route<S> {
    type Request = HandlerRequest<S>;
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = RouteService<S>;
    type Future = CreateRouteService<S>;

    fn new_service(&self) -> Self::Future {
        CreateRouteService {
            fut: self.service.new_service(),
            filters: self.filters.clone(),
        }
    }
}

type RouteFuture<S> =
    Box<Future<Item = BoxedRouteService<HandlerRequest<S>, Response>, Error = ()>>;

pub struct CreateRouteService<S> {
    fut: RouteFuture<S>,
    filters: Rc<Vec<Box<Filter<S>>>>,
}

impl<S: 'static> Future for CreateRouteService<S> {
    type Item = RouteService<S>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.fut.poll()? {
            Async::Ready(service) => Ok(Async::Ready(RouteService {
                service,
                filters: self.filters.clone(),
            })),
            Async::NotReady => Ok(Async::NotReady),
        }
    }
}

pub struct RouteService<S> {
    service: BoxedRouteService<HandlerRequest<S>, Response>,
    filters: Rc<Vec<Box<Filter<S>>>>,
}

impl<S> RouteService<S> {
    pub fn check(&self, req: &mut ServiceRequest<S>) -> bool {
        for f in self.filters.iter() {
            if !f.check(req) {
                return false;
            }
        }
        true
    }
}

impl<S> Service for RouteService<S> {
    type Request = HandlerRequest<S>;
    type Response = Response;
    type Error = Error;
    type Future = Box<Future<Item = Response, Error = Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        self.service.call(req)
    }
}

pub struct RouteBuilder<S> {
    filters: Vec<Box<Filter<S>>>,
}

impl<S: 'static> RouteBuilder<S> {
    fn new() -> RouteBuilder<S> {
        RouteBuilder {
            filters: Vec::new(),
        }
    }

    /// Add method match filter to the route.
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// # use actix_web::*;
    /// # fn main() {
    /// App::new().resource("/path", |r| {
    ///     r.route()
    ///         .filter(pred::Get())
    ///         .filter(pred::Header("content-type", "text/plain"))
    ///         .f(|req| HttpResponse::Ok())
    /// })
    /// #      .finish();
    /// # }
    /// ```
    pub fn method(mut self, method: Method) -> Self {
        self.filters.push(Box::new(filter::Method(method)));
        self
    }

    /// Add filter to the route.
    ///
    /// ```rust
    /// # extern crate actix_web;
    /// # use actix_web::*;
    /// # fn main() {
    /// App::new().resource("/path", |r| {
    ///     r.route()
    ///         .filter(pred::Get())
    ///         .filter(pred::Header("content-type", "text/plain"))
    ///         .f(|req| HttpResponse::Ok())
    /// })
    /// #      .finish();
    /// # }
    /// ```
    pub fn filter<F: Filter<S> + 'static>(&mut self, f: F) -> &mut Self {
        self.filters.push(Box::new(f));
        self
    }

    // pub fn map<T, U, F: IntoNewService<T>>(
    //     self,
    //     md: F,
    // ) -> RouteServiceBuilder<T, S, (), U>
    // where
    //     T: NewService<
    //         Request = HandlerRequest<S>,
    //         Response = HandlerRequest<S, U>,
    //         InitError = (),
    //     >,
    // {
    //     RouteServiceBuilder {
    //         service: md.into_new_service(),
    //         filters: self.filters,
    //         _t: PhantomData,
    //     }
    // }

    /// Set handler function, use request extractor for parameters.
    ///
    /// ```rust
    /// # extern crate bytes;
    /// # extern crate actix_web;
    /// # extern crate futures;
    /// #[macro_use] extern crate serde_derive;
    /// use actix_web::{http, App, Path, Result};
    ///
    /// #[derive(Deserialize)]
    /// struct Info {
    ///     username: String,
    /// }
    ///
    /// /// extract path info using serde
    /// fn index(info: Path<Info>) -> Result<String> {
    ///     Ok(format!("Welcome {}!", info.username))
    /// }
    ///
    /// fn main() {
    ///     let app = App::new().resource(
    ///         "/{username}/index.html", // <- define path parameters
    ///         |r| r.method(http::Method::GET).with(index),
    ///     ); // <- use `with` extractor
    /// }
    /// ```
    ///
    /// It is possible to use multiple extractors for one handler function.
    ///
    /// ```rust
    /// # extern crate bytes;
    /// # extern crate actix_web;
    /// # extern crate futures;
    /// #[macro_use] extern crate serde_derive;
    /// # use std::collections::HashMap;
    /// use actix_web::{http, App, Json, Path, Query, Result};
    ///
    /// #[derive(Deserialize)]
    /// struct Info {
    ///     username: String,
    /// }
    ///
    /// /// extract path info using serde
    /// fn index(
    ///     path: Path<Info>, query: Query<HashMap<String, String>>, body: Json<Info>,
    /// ) -> Result<String> {
    ///     Ok(format!("Welcome {}!", path.username))
    /// }
    ///
    /// fn main() {
    ///     let app = App::new().resource(
    ///         "/{username}/index.html", // <- define path parameters
    ///         |r| r.method(http::Method::GET).with(index),
    ///     ); // <- use `with` extractor
    /// }
    /// ```
    pub fn to<F, P, R>(self, handler: F) -> Route<S>
    where
        F: Factory<S, (), P, R> + 'static,
        P: FromRequest<S> + 'static,
        R: Responder<S> + 'static,
    {
        Route {
            service: Box::new(RouteNewService::new(
                Extract::new().and_then(Handle::new(handler)),
            )),
            filters: Rc::new(self.filters),
        }
    }

    /// Set async handler function, use request extractor for parameters.
    /// Also this method needs to be used if your handler function returns
    /// `impl Future<>`
    ///
    /// ```rust
    /// # extern crate bytes;
    /// # extern crate actix_web;
    /// # extern crate futures;
    /// #[macro_use] extern crate serde_derive;
    /// use actix_web::{http, App, Error, Path};
    /// use futures::Future;
    ///
    /// #[derive(Deserialize)]
    /// struct Info {
    ///     username: String,
    /// }
    ///
    /// /// extract path info using serde
    /// fn index(info: Path<Info>) -> Box<Future<Item = &'static str, Error = Error>> {
    ///     unimplemented!()
    /// }
    ///
    /// fn main() {
    ///     let app = App::new().resource(
    ///         "/{username}/index.html", // <- define path parameters
    ///         |r| r.method(http::Method::GET).with_async(index),
    ///     ); // <- use `with` extractor
    /// }
    /// ```
    #[allow(clippy::wrong_self_convention)]
    pub fn to_async<F, P, R>(self, handler: F) -> Route<S>
    where
        F: AsyncFactory<S, (), P, R>,
        P: FromRequest<S> + 'static,
        R: IntoFuture + 'static,
        R::Item: Into<Response>,
        R::Error: Into<Error>,
    {
        Route {
            service: Box::new(RouteNewService::new(
                Extract::new().then(AsyncHandle::new(handler)),
            )),
            filters: Rc::new(self.filters),
        }
    }
}

pub struct RouteServiceBuilder<T, S, U1, U2> {
    service: T,
    filters: Vec<Box<Filter<S>>>,
    _t: PhantomData<(U1, U2)>,
}

// impl<T, S: 'static, U1, U2> RouteServiceBuilder<T, S, U1, U2>
// where
//     T: NewService<
//         Request = HandlerRequest<S, U1>,
//         Response = HandlerRequest<S, U2>,
//         Error = Error,
//         InitError = (),
//     >,
// {
//     pub fn new<F: IntoNewService<T>>(factory: F) -> Self {
//         RouteServiceBuilder {
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
//     ) -> RouteServiceBuilder<
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
//         RouteServiceBuilder {
//             service: self
//                 .service
//                 .and_then(md.into_new_service().map_err(|e| e.into())),
//             filters: self.filters,
//             _t: PhantomData,
//         }
//     }

//     pub fn to_async<F, P, R>(self, handler: F) -> Route<S>
//     where
//         F: AsyncFactory<S, U2, P, R>,
//         P: FromRequest<S> + 'static,
//         R: IntoFuture,
//         R::Item: Into<Response>,
//         R::Error: Into<Error>,
//     {
//         Route {
//             service: self
//                 .service
//                 .and_then(Extract::new(P::Config::default()))
//                 .then(AsyncHandle::new(handler)),
//             filters: Rc::new(self.filters),
//         }
//     }

//     pub fn to<F, P, R>(self, handler: F) -> Route<S>
//     where
//         F: Factory<S, U2, P, R> + 'static,
//         P: FromRequest<S> + 'static,
//         R: Responder<S> + 'static,
//     {
//         Route {
//             service: Box::new(RouteNewService::new(
//                 self.service
//                     .and_then(Extract::new(P::Config::default()))
//                     .and_then(Handle::new(handler)),
//             )),
//             filters: Rc::new(self.filters),
//         }
//     }
// }

struct RouteNewService<
    S,
    T: NewService<Request = HandlerRequest<S, U>, Error = Error>,
    U,
>(T, PhantomData<(S, U)>);

impl<S, T, U> RouteNewService<S, T, U>
where
    T: NewService<Request = HandlerRequest<S, U>, Response = Response, Error = Error>,
    T::Future: 'static,
    <T::Service as Service>::Future: 'static,
{
    pub fn new(service: T) -> Self {
        RouteNewService(service, PhantomData)
    }
}

impl<S, T, U> NewService for RouteNewService<S, T, U>
where
    S: 'static,
    U: 'static,
    T: NewService<Request = HandlerRequest<S, U>, Response = Response, Error = Error>,
    T::Future: 'static,
    T::Service: 'static,
    <T::Service as Service>::Future: 'static,
{
    type Request = HandlerRequest<S, U>;
    type Response = Response;
    type Error = Error;
    type InitError = ();
    type Service = BoxedRouteService<Self::Request, Self::Response>;
    type Future = Box<Future<Item = Self::Service, Error = Self::InitError>>;

    fn new_service(&self) -> Self::Future {
        Box::new(self.0.new_service().map_err(|_| ()).and_then(|service| {
            let service: BoxedRouteService<_, _> = Box::new(RouteServiceWrapper {
                service,
                _t: PhantomData,
            });
            Ok(service)
        }))
    }
}

struct RouteServiceWrapper<S, T: Service, U> {
    service: T,
    _t: PhantomData<(S, U)>,
}

impl<S, T, U> Service for RouteServiceWrapper<S, T, U>
where
    S: 'static,
    T::Future: 'static,
    T: Service<Request = HandlerRequest<S, U>, Response = Response, Error = Error>,
{
    type Request = HandlerRequest<S, U>;
    type Response = Response;
    type Error = Error;
    type Future = Box<Future<Item = Response, Error = Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: HandlerRequest<S, U>) -> Self::Future {
        Box::new(self.service.call(req))
    }
}
