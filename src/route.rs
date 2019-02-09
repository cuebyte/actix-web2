use std::marker::PhantomData;

use actix_http::{http::Method, Error, Response};
use actix_service::{IntoNewService, NewService, Service};
use futures::{try_ready, Async, Future, IntoFuture, Poll};

use crate::app::HttpServiceFactory;
use crate::handler::{
    AsyncFactory, AsyncHandle, Extract, Factory, FromRequest, Handle, HandlerRequest,
};
use crate::responder::Responder;
use crate::service::{ServiceRequest, ServiceResponse};

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct Route<T, S = ()> {
    service: T,
    pattern: String,
    methods: Vec<Method>,
    state: PhantomData<S>,
}

impl<S> Route<(), S> {
    pub fn build(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder::new(path)
    }

    pub fn get(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder::new(path).method(Method::GET)
    }

    pub fn post(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder::new(path).method(Method::POST)
    }

    pub fn put(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder::new(path).method(Method::PUT)
    }

    pub fn delete(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder::new(path).method(Method::DELETE)
    }
}

impl<T, S> Route<T, S>
where
    T: NewService<Request = ServiceRequest<S>, Response = Response> + 'static,
    T::Error: Into<Error>,
{
    pub fn new<F: IntoNewService<T>>(pattern: &str, factory: F) -> Self {
        Route {
            pattern: pattern.to_owned(),
            service: factory.into_new_service(),
            methods: Vec::new(),
            state: PhantomData,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }
}

impl<T, S> HttpServiceFactory<ServiceRequest<S>> for Route<T, S>
where
    T: NewService<Request = HandlerRequest<S>, Response = Response, Error = Error>
        + 'static,
    T::Service: 'static,
{
    type Factory = RouteFactory<T, S>;

    fn path(&self) -> &str {
        &self.pattern
    }

    fn create(self) -> Self::Factory {
        RouteFactory {
            service: self.service,
            pattern: self.pattern,
            methods: self.methods,
            state: PhantomData,
        }
    }
}

pub struct RouteFactory<T, S> {
    service: T,
    pattern: String,
    methods: Vec<Method>,
    state: PhantomData<S>,
}

impl<T, S> NewService for RouteFactory<T, S>
where
    T: NewService<Request = HandlerRequest<S>, Response = Response, Error = Error>
        + 'static,
    T::Service: 'static,
{
    type Request = ServiceRequest<S>;
    type Response = ServiceResponse;
    type Error = T::Error;
    type InitError = T::InitError;
    type Service = RouteService<T::Service, S>;
    type Future = CreateRouteService<T, S>;

    fn new_service(&self) -> Self::Future {
        CreateRouteService {
            fut: self.service.new_service(),
            pattern: self.pattern.clone(),
            methods: self.methods.clone(),
            state: PhantomData,
        }
    }
}

pub struct CreateRouteService<T: NewService<Request = HandlerRequest<S>>, S> {
    fut: T::Future,
    pattern: String,
    methods: Vec<Method>,
    state: PhantomData<S>,
}

impl<T, S> Future for CreateRouteService<T, S>
where
    T: NewService<Request = HandlerRequest<S>, Response = Response>,
    T::Error: Into<Error>,
{
    type Item = RouteService<T::Service, S>;
    type Error = T::InitError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let service = try_ready!(self.fut.poll());

        Ok(Async::Ready(RouteService {
            service,
            pattern: self.pattern.clone(),
            methods: self.methods.clone(),
            state: PhantomData,
        }))
    }
}

pub struct RouteService<T, S> {
    service: T,
    pattern: String,
    methods: Vec<Method>,
    state: PhantomData<S>,
}

impl<T, S> Service for RouteService<T, S>
where
    T: Service<Request = HandlerRequest<S>, Response = Response> + 'static,
{
    type Request = ServiceRequest<S>;
    type Response = ServiceResponse;
    type Error = T::Error;
    type Future = RouteServiceResponse<T, S>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: ServiceRequest<S>) -> Self::Future {
        RouteServiceResponse {
            fut: self.service.call(HandlerRequest::new(req.into_request())),
            _t: PhantomData,
        }
    }
}

pub struct RouteServiceResponse<T: Service, S> {
    fut: T::Future,
    _t: PhantomData<S>,
}

impl<T, S> Future for RouteServiceResponse<T, S>
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

pub struct RoutePatternBuilder<S> {
    pattern: String,
    methods: Vec<Method>,
    state: PhantomData<S>,
}

impl<S> RoutePatternBuilder<S> {
    fn new(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder {
            pattern: path.to_string(),
            methods: Vec::new(),
            state: PhantomData,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }

    pub fn and_then<T, U, F: IntoNewService<T>>(self, md: F) -> RouteBuilder<T, S, (), U>
    where
        T: NewService<
            Request = HandlerRequest<S>,
            Response = HandlerRequest<S, U>,
            InitError = (),
        >,
    {
        RouteBuilder {
            service: md.into_new_service(),
            pattern: self.pattern,
            methods: self.methods,
            _t: PhantomData,
        }
    }

    pub fn finish<F, P, R>(
        self,
        handler: F,
    ) -> Route<
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
        Route {
            service: Extract::new(P::Config::default()).and_then(Handle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            state: PhantomData,
        }
    }

    pub fn with<F, P, R>(
        self,
        handler: F,
    ) -> Route<
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
        Route {
            service: Extract::new(P::Config::default()).then(AsyncHandle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            state: PhantomData,
        }
    }
}

pub struct RouteBuilder<T, S, U1, U2> {
    service: T,
    pattern: String,
    methods: Vec<Method>,
    _t: PhantomData<(S, U1, U2)>,
}

impl<T, S, U1, U2> RouteBuilder<T, S, U1, U2>
where
    T: NewService<
        Request = HandlerRequest<S, U1>,
        Response = HandlerRequest<S, U2>,
        Error = Error,
        InitError = (),
    >,
{
    pub fn new<F: IntoNewService<T>>(path: &str, factory: F) -> Self {
        RouteBuilder {
            service: factory.into_new_service(),
            pattern: path.to_string(),
            methods: Vec::new(),
            _t: PhantomData,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }

    pub fn and_then<T1, U3, F: IntoNewService<T1>>(
        self,
        md: F,
    ) -> RouteBuilder<
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
        RouteBuilder {
            service: self
                .service
                .and_then(md.into_new_service().map_err(|e| e.into())),
            pattern: self.pattern,
            methods: self.methods,
            _t: PhantomData,
        }
    }

    pub fn with<F, P, R>(
        self,
        handler: F,
    ) -> Route<
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
        Route {
            service: self
                .service
                .and_then(Extract::new(P::Config::default()))
                .then(AsyncHandle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            state: PhantomData,
        }
    }

    pub fn finish<F, P, R>(
        self,
        handler: F,
    ) -> Route<
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
        Route {
            service: self
                .service
                .and_then(Extract::new(P::Config::default()))
                .and_then(Handle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            state: PhantomData,
        }
    }
}
