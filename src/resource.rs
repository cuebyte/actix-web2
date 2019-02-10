use std::marker::PhantomData;
use std::rc::Rc;

use actix_http::{http::Method, Error, Response};
use actix_router::ResourceDef;
use actix_service::{IntoNewService, NewService, Service};
use futures::future::{ok, Either, FutureResult};
use futures::{try_ready, Async, Future, IntoFuture, Poll};

use crate::app::HttpServiceFactory;
use crate::filter::{self, Filter};
use crate::handler::{
    AsyncFactory, AsyncHandle, Extract, Factory, FromRequest, Handle, HandlerRequest,
};
use crate::responder::Responder;
use crate::service::{ServiceRequest, ServiceResponse};

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct Resource<T, S = ()> {
    service: T,
    pattern: ResourceDef,
    filters: Vec<Box<Filter<S>>>,
}

impl<S: 'static> Resource<(), S> {
    pub fn build<R: Into<ResourceDef>>(rdef: R) -> ResourceBuilder<S> {
        ResourceBuilder::new(rdef.into())
    }

    pub fn get<R: Into<ResourceDef>>(rdef: R) -> ResourceBuilder<S> {
        ResourceBuilder::new(rdef.into()).method(Method::GET)
    }

    pub fn post<R: Into<ResourceDef>>(rdef: R) -> ResourceBuilder<S> {
        ResourceBuilder::new(rdef.into()).method(Method::POST)
    }

    pub fn put<R: Into<ResourceDef>>(rdef: R) -> ResourceBuilder<S> {
        ResourceBuilder::new(rdef.into()).method(Method::PUT)
    }

    pub fn delete<R: Into<ResourceDef>>(rdef: R) -> ResourceBuilder<S> {
        ResourceBuilder::new(rdef.into()).method(Method::DELETE)
    }
}

impl<T, S> Resource<T, S>
where
    T: NewService<Request = ServiceRequest<S>, Response = Response> + 'static,
    T::Error: Into<Error>,
{
    pub fn new<F: IntoNewService<T>>(rdef: ResourceDef, factory: F) -> Self {
        Resource {
            pattern: rdef,
            service: factory.into_new_service(),
            filters: Vec::new(),
        }
    }
}

impl<T, S> HttpServiceFactory<ServiceRequest<S>> for Resource<T, S>
where
    T: NewService<Request = HandlerRequest<S>, Response = Response, Error = Error>
        + 'static,
    T::Service: 'static,
{
    type Factory = ResourceFactory<T, S>;

    fn rdef(&self) -> &ResourceDef {
        &self.pattern
    }

    fn create(self) -> Self::Factory {
        ResourceFactory {
            service: self.service,
            pattern: self.pattern,
            filters: Rc::new(self.filters),
        }
    }
}

pub struct ResourceFactory<T, S> {
    service: T,
    pattern: ResourceDef,
    filters: Rc<Vec<Box<Filter<S>>>>,
}

impl<T, S> NewService for ResourceFactory<T, S>
where
    T: NewService<Request = HandlerRequest<S>, Response = Response, Error = Error>
        + 'static,
    T::Service: 'static,
{
    type Request = ServiceRequest<S>;
    type Response = ServiceResponse;
    type Error = T::Error;
    type InitError = T::InitError;
    type Service = ResourceService<T::Service, S>;
    type Future = CreateRouteService<T, S>;

    fn new_service(&self) -> Self::Future {
        CreateRouteService {
            fut: self.service.new_service(),
            filters: self.filters.clone(),
        }
    }
}

pub struct CreateRouteService<T: NewService<Request = HandlerRequest<S>>, S> {
    fut: T::Future,
    filters: Rc<Vec<Box<Filter<S>>>>,
}

impl<T, S> Future for CreateRouteService<T, S>
where
    T: NewService<Request = HandlerRequest<S>, Response = Response>,
    T::Error: Into<Error>,
{
    type Item = ResourceService<T::Service, S>;
    type Error = T::InitError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let service = try_ready!(self.fut.poll());

        Ok(Async::Ready(ResourceService {
            service,
            filters: self.filters.clone(),
        }))
    }
}

pub struct ResourceService<T, S> {
    service: T,
    filters: Rc<Vec<Box<Filter<S>>>>,
}

impl<T, S> Service for ResourceService<T, S>
where
    T: Service<Request = HandlerRequest<S>, Response = Response> + 'static,
{
    type Request = ServiceRequest<S>;
    type Response = ServiceResponse;
    type Error = T::Error;
    type Future =
        Either<ResourceServiceResponse<T, S>, FutureResult<Self::Response, Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, mut req: ServiceRequest<S>) -> Self::Future {
        for f in self.filters.iter() {
            if !f.check(&mut req) {
                return Either::B(ok(req.into()));
            }
        }

        Either::A(ResourceServiceResponse {
            fut: self.service.call(HandlerRequest::new(req.into_request())),
            _t: PhantomData,
        })
    }
}

pub struct ResourceServiceResponse<T: Service, S> {
    fut: T::Future,
    _t: PhantomData<S>,
}

impl<T, S> Future for ResourceServiceResponse<T, S>
where
    T: Service<Request = HandlerRequest<S>, Response = Response>,
{
    type Item = ServiceResponse;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let res = try_ready!(self.fut.poll());
        Ok(Async::Ready(res.into()))
    }
}

pub struct ResourceBuilder<S> {
    pattern: ResourceDef,
    filters: Vec<Box<Filter<S>>>,
}

impl<S: 'static> ResourceBuilder<S> {
    fn new(rdef: ResourceDef) -> ResourceBuilder<S> {
        ResourceBuilder {
            pattern: rdef,
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

    pub fn map<T, U, F: IntoNewService<T>>(
        self,
        md: F,
    ) -> ResourceServiceBuilder<T, S, (), U>
    where
        T: NewService<
            Request = HandlerRequest<S>,
            Response = HandlerRequest<S, U>,
            InitError = (),
        >,
    {
        ResourceServiceBuilder {
            service: md.into_new_service(),
            pattern: self.pattern,
            filters: self.filters,
            _t: PhantomData,
        }
    }

    pub fn to<F, P, R>(
        self,
        handler: F,
    ) -> Resource<
        impl NewService<
            Request = HandlerRequest<S>,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: Factory<S, (), P, R> + 'static,
        P: FromRequest<S> + 'static,
        R: Responder<S> + 'static,
    {
        Resource {
            service: Extract::new(P::Config::default()).and_then(Handle::new(handler)),
            pattern: self.pattern,
            filters: self.filters,
        }
    }

    pub fn to_async<F, P, R>(
        self,
        handler: F,
    ) -> Resource<
        impl NewService<
            Request = HandlerRequest<S>,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: AsyncFactory<S, (), P, R>,
        P: FromRequest<S> + 'static,
        R: IntoFuture,
        R::Item: Into<Response>,
        R::Error: Into<Error>,
    {
        Resource {
            service: Extract::new(P::Config::default()).then(AsyncHandle::new(handler)),
            pattern: self.pattern,
            filters: self.filters,
        }
    }
}

pub struct ResourceServiceBuilder<T, S, U1, U2> {
    service: T,
    pattern: ResourceDef,
    filters: Vec<Box<Filter<S>>>,
    _t: PhantomData<(U1, U2)>,
}

impl<T, S: 'static, U1, U2> ResourceServiceBuilder<T, S, U1, U2>
where
    T: NewService<
        Request = HandlerRequest<S, U1>,
        Response = HandlerRequest<S, U2>,
        Error = Error,
        InitError = (),
    >,
{
    pub fn new<F: IntoNewService<T>>(rdef: ResourceDef, factory: F) -> Self {
        ResourceServiceBuilder {
            service: factory.into_new_service(),
            pattern: rdef,
            filters: Vec::new(),
            _t: PhantomData,
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

    pub fn map<T1, U3, F: IntoNewService<T1>>(
        self,
        md: F,
    ) -> ResourceServiceBuilder<
        impl NewService<
            Request = HandlerRequest<S, U1>,
            Response = HandlerRequest<S, U3>,
            Error = Error,
            InitError = (),
        >,
        S,
        U1,
        U2,
    >
    where
        T1: NewService<
            Request = HandlerRequest<S, U2>,
            Response = HandlerRequest<S, U3>,
            InitError = (),
        >,
        T1::Error: Into<Error>,
    {
        ResourceServiceBuilder {
            service: self
                .service
                .and_then(md.into_new_service().map_err(|e| e.into())),
            pattern: self.pattern,
            filters: self.filters,
            _t: PhantomData,
        }
    }

    pub fn to_async<F, P, R>(
        self,
        handler: F,
    ) -> Resource<
        impl NewService<
            Request = HandlerRequest<S, U1>,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: AsyncFactory<S, U2, P, R>,
        P: FromRequest<S> + 'static,
        R: IntoFuture,
        R::Item: Into<Response>,
        R::Error: Into<Error>,
    {
        Resource {
            service: self
                .service
                .and_then(Extract::new(P::Config::default()))
                .then(AsyncHandle::new(handler)),
            pattern: self.pattern,
            filters: self.filters,
        }
    }

    pub fn to<F, P, R>(
        self,
        handler: F,
    ) -> Resource<
        impl NewService<
            Request = HandlerRequest<S, U1>,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: Factory<S, U2, P, R> + 'static,
        P: FromRequest<S> + 'static,
        R: Responder<S> + 'static,
    {
        Resource {
            service: self
                .service
                .and_then(Extract::new(P::Config::default()))
                .and_then(Handle::new(handler)),
            pattern: self.pattern,
            filters: self.filters,
        }
    }
}
