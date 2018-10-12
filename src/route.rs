use futures::{Async, Future, Poll};

use actix_http::http::{HeaderName, HeaderValue, Method};
use actix_http::{Error, Request, Response, ResponseError};
use actix_net::service::{IntoNewService, NewService, NewServiceExt, Service};

use super::handler::{FromRequest, Responder};
use super::param::Params;
use super::pattern::ResourcePattern;
use super::request::Request as WebRequest;
use super::router::HttpService;
use super::with::{Extract, Handle, WithFactory};

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct Route<T> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl Route<()> {
    pub fn build(path: &str) -> RoutePatternBuilder {
        RoutePatternBuilder::new(path)
    }
}

impl<T> Route<T>
where
    T: NewService<Request = WebRequest, Response = Response> + 'static,
{
    pub fn new<F: IntoNewService<T>>(pattern: ResourcePattern, factory: F) -> Self {
        Route {
            pattern,
            service: factory.into_new_service(),
            headers: Vec::new(),
            methods: Vec::new(),
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

impl<T> NewService for Route<T>
where
    T: NewService<Request = WebRequest, Response = Response> + 'static,
{
    type Request = Request;
    type Response = T::Response;
    type Error = T::Error;
    type InitError = T::InitError;
    type Service = RouteService<T::Service>;
    type Future = CreateRouteService<T>;

    fn new_service(&self) -> Self::Future {
        CreateRouteService {
            fut: self.service.new_service(),
            pattern: self.pattern.clone(),
            methods: self.methods.clone(),
            headers: self.headers.clone(),
        }
    }
}

pub struct CreateRouteService<T: NewService> {
    fut: T::Future,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl<T> Future for CreateRouteService<T>
where
    T: NewService<Request = WebRequest, Response = Response>,
{
    type Item = RouteService<T::Service>;
    type Error = T::InitError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let service = try_ready!(self.fut.poll());

        Ok(Async::Ready(RouteService {
            service,
            pattern: self.pattern.clone(),
            methods: self.methods.clone(),
            headers: self.headers.clone(),
        }))
    }
}

pub struct RouteService<T> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl<T> Service for RouteService<T>
where
    T: Service<Request = WebRequest, Response = Response> + 'static,
{
    type Request = Request;
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        self.service.call(WebRequest::new(req, Params::new()))
    }
}

impl<T> HttpService for RouteService<T>
where
    T: Service<Request = WebRequest, Response = Response> + 'static,
{
    fn handle(&mut self, req: Request) -> Result<Self::Future, Request> {
        if self.methods.is_empty()
            || !self.methods.is_empty() && self.methods.contains(req.method())
        {
            if let Some(params) = self.pattern.match_with_params(&req, 0) {
                return Ok(self.service.call(WebRequest::new(req, params)));
            }
        }
        Err(req)
    }
}

pub struct RoutePatternBuilder {
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl RoutePatternBuilder {
    fn new(path: &str) -> RoutePatternBuilder {
        RoutePatternBuilder {
            pattern: ResourcePattern::new(path),
            methods: Vec::new(),
            headers: Vec::new(),
        }
    }
    pub fn middleware<T, F: IntoNewService<T>>(self, md: F) -> RouteBuilder<T>
    where
        T: NewService<Request = WebRequest, Response = WebRequest, InitError = ()>,
    {
        RouteBuilder {
            service: md.into_new_service(),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
        }
    }

    pub fn finish<F, P, R>(
        self, handler: F,
    ) -> Route<
        impl NewService<
            Request = WebRequest,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
    >
    where
        F: WithFactory<P, R> + 'static,
        P: FromRequest + 'static,
        R: Responder + 'static,
    {
        Route {
            service: Extract::new(P::Config::default()).and_then(Handle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
        }
    }
}

pub struct RouteBuilder<T> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl<T> RouteBuilder<T>
where
    T: NewService<Request = WebRequest, Response = WebRequest, InitError = ()>,
{
    pub fn new<F: IntoNewService<T>>(path: &str, factory: F) -> Self {
        RouteBuilder {
            service: factory.into_new_service(),
            pattern: ResourcePattern::new(path),
            methods: Vec::new(),
            headers: Vec::new(),
        }
    }

    pub fn middleware<S, F: IntoNewService<S>>(
        self, md: F,
    ) -> RouteBuilder<
        impl NewService<
            Request = WebRequest,
            Response = WebRequest,
            Error = S::Error,
            InitError = (),
        >,
    >
    where
        S: NewService<Request = WebRequest, Response = WebRequest, InitError = ()>,
        S::Error: From<T::Error>,
    {
        RouteBuilder {
            service: self.service.from_err().and_then(md.into_new_service()),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
        }
    }

    pub fn finish<F, P, R>(
        self, handler: F,
    ) -> Route<
        impl NewService<
            Request = WebRequest,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
    >
    where
        F: WithFactory<P, R> + 'static,
        P: FromRequest + 'static,
        R: Responder + 'static,
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
        }
    }
}
