use std::marker::PhantomData;

use futures::{Async, Future, Poll};

use actix_http::http::{HeaderName, HeaderValue, Method};
use actix_http::{Error, Request, Response, ResponseError};
use actix_net::service::{IntoNewService, NewService, NewServiceExt, Service};

use super::app::{HttpService, HttpServiceFactory, State};
use super::handler::{Extract, Factory, FromRequest, Handle};
use super::param::Params;
use super::pattern::ResourcePattern;
use super::request::Request as WebRequest;
use super::responder::Responder;

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct Route<T, S = ()> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: PhantomData<S>,
}

impl<S> Route<(), S> {
    pub fn build(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder::new(path)
    }
}

impl<T, S> Route<T, S>
where
    T: NewService<Request = WebRequest<S>, Response = Response> + 'static,
{
    pub fn new<F: IntoNewService<T>>(pattern: ResourcePattern, factory: F) -> Self {
        Route {
            pattern,
            service: factory.into_new_service(),
            headers: Vec::new(),
            methods: Vec::new(),
            state: PhantomData,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }

    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push((name, value));
        self
    }
}

impl<T, S> HttpServiceFactory<S> for Route<T, S>
where
    T: NewService<Request = WebRequest<S>, Response = Response> + 'static,
{
    type Factory = RouteFactory<T, S>;

    fn create(self, state: State<S>) -> Self::Factory {
        RouteFactory {
            state,
            service: self.service,
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
        }
    }
}

pub struct RouteFactory<T, S> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
}

impl<T, S> NewService for RouteFactory<T, S>
where
    T: NewService<Request = WebRequest<S>, Response = Response> + 'static,
{
    type Request = Request;
    type Response = T::Response;
    type Error = T::Error;
    type InitError = T::InitError;
    type Service = RouteService<T::Service, S>;
    type Future = CreateRouteService<T, S>;

    fn new_service(&self) -> Self::Future {
        CreateRouteService {
            fut: self.service.new_service(),
            pattern: self.pattern.clone(),
            methods: self.methods.clone(),
            headers: self.headers.clone(),
            state: self.state.clone(),
        }
    }
}

pub struct CreateRouteService<T: NewService, S> {
    fut: T::Future,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
}

impl<T, S> Future for CreateRouteService<T, S>
where
    T: NewService<Request = WebRequest<S>, Response = Response>,
{
    type Item = RouteService<T::Service, S>;
    type Error = T::InitError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let service = try_ready!(self.fut.poll());

        Ok(Async::Ready(RouteService {
            service,
            state: self.state.clone(),
            pattern: self.pattern.clone(),
            methods: self.methods.clone(),
            headers: self.headers.clone(),
        }))
    }
}

pub struct RouteService<T, S> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
}

impl<T, S> Service for RouteService<T, S>
where
    T: Service<Request = WebRequest<S>, Response = Response> + 'static,
{
    type Request = Request;
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        self.service
            .call(WebRequest::new(self.state.clone(), req, Params::new()))
    }
}

impl<T, S> HttpService for RouteService<T, S>
where
    T: Service<Request = WebRequest<S>, Response = Response> + 'static,
    S: 'static,
{
    fn handle(&mut self, req: Request) -> Result<Self::Future, Request> {
        if self.methods.is_empty()
            || !self.methods.is_empty() && self.methods.contains(req.method())
        {
            if let Some(params) = self.pattern.match_with_params(&req, 0) {
                return Ok(self.service.call(WebRequest::new(
                    self.state.clone(),
                    req,
                    params,
                )));
            }
        }
        Err(req)
    }
}

pub struct RoutePatternBuilder<S> {
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: PhantomData<S>,
}

impl<S> RoutePatternBuilder<S> {
    fn new(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder {
            pattern: ResourcePattern::new(path),
            methods: Vec::new(),
            headers: Vec::new(),
            state: PhantomData,
        }
    }
    pub fn middleware<T, F: IntoNewService<T>>(self, md: F) -> RouteBuilder<T, S>
    where
        T: NewService<Request = WebRequest<S>, Response = WebRequest<S>, InitError = ()>,
    {
        RouteBuilder {
            service: md.into_new_service(),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }

    pub fn finish<F, P, R>(
        self, handler: F,
    ) -> Route<
        impl NewService<
            Request = WebRequest<S>,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: Factory<S, P, R> + 'static,
        P: FromRequest<S> + 'static,
        R: Responder<S> + 'static,
    {
        Route {
            service: Extract::new(P::Config::default()).and_then(Handle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }
}

pub struct RouteBuilder<T, S> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: PhantomData<S>,
}

impl<T, S> RouteBuilder<T, S>
where
    T: NewService<Request = WebRequest<S>, Response = WebRequest<S>, InitError = ()>,
{
    pub fn new<F: IntoNewService<T>>(path: &str, factory: F) -> Self {
        RouteBuilder {
            service: factory.into_new_service(),
            pattern: ResourcePattern::new(path),
            methods: Vec::new(),
            headers: Vec::new(),
            state: PhantomData,
        }
    }

    pub fn middleware<U, F: IntoNewService<U>>(
        self, md: F,
    ) -> RouteBuilder<
        impl NewService<
            Request = WebRequest<S>,
            Response = WebRequest<S>,
            Error = U::Error,
            InitError = (),
        >,
        S,
    >
    where
        U: NewService<Request = WebRequest<S>, Response = WebRequest<S>, InitError = ()>,
        U::Error: From<T::Error>,
    {
        RouteBuilder {
            service: self.service.from_err().and_then(md.into_new_service()),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }

    pub fn finish<F, P, R>(
        self, handler: F,
    ) -> Route<
        impl NewService<
            Request = WebRequest<S>,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: Factory<S, P, R> + 'static,
        P: FromRequest<S> + 'static,
        R: Responder<S> + 'static,
        T::Error: ResponseError,
    {
        Route {
            service: self
                .service
                .from_err()
                .and_then(Extract::new(P::Config::default()))
                .and_then(Handle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }
}
