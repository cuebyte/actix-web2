use std::marker::PhantomData;

use actix_http::http::{HeaderName, HeaderValue, Method};
use actix_http::{Error, Request, Response};
use actix_net::service::{IntoNewService, NewService, NewServiceExt, Service};
use actix_router::Path;
use futures::{try_ready, Async, Future, IntoFuture, Poll};

use super::app::{HttpServiceFactory, State};
use super::handler::{
    AsyncFactory, AsyncHandle, Extract, Factory, FromRequest, Handle, ServiceRequest,
};
// use super::param::Params;
use super::request::Request as WebRequest;
use super::responder::Responder;
use super::url::Url;

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct Route<T, S = ()> {
    service: T,
    pattern: String,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
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

impl<T, S> HttpServiceFactory<S, Request> for Route<T, S>
where
    T: NewService<Request = ServiceRequest<S>, Response = Response, Error = Error>
        + 'static,
    T::Service: 'static,
{
    type Factory = RouteFactory<T, S>;

    fn path(&self) -> &str {
        &self.pattern
    }

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
    pattern: String,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
}

impl<T, S> NewService for RouteFactory<T, S>
where
    T: NewService<Request = ServiceRequest<S>, Response = Response, Error = Error>
        + 'static,
    T::Service: 'static,
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

pub struct CreateRouteService<T: NewService<Request = ServiceRequest<S>>, S> {
    fut: T::Future,
    pattern: String,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
}

impl<T, S> Future for CreateRouteService<T, S>
where
    T: NewService<Request = ServiceRequest<S>, Response = Response>,
    T::Error: Into<Error>,
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
    pattern: String,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
}

impl<T, S> Service for RouteService<T, S>
where
    T: Service<Request = ServiceRequest<S>, Response = Response, Error = Error>
        + 'static,
{
    type Request = Request;
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: Request) -> Self::Future {
        self.service.call(ServiceRequest::new(WebRequest::new(
            self.state.clone(),
            req,
            Path::new(Url::default()),
        )))
    }
}

// impl<T, S> HttpService<Request> for RouteService<T, S>
// where
//     T: Service<ServiceRequest<S>, Response = Response, Error = Error> + 'static,
//     S: 'static,
// {
//     fn handle(&mut self, req: Request) -> Result<Self::Future, Request> {
//         if self.methods.is_empty()
//             || !self.methods.is_empty() && self.methods.contains(req.method())
//         {
//             if let Some(params) = self.pattern.match_with_params(&req, 0) {
//                 return Ok(self.service.call(ServiceRequest::new(WebRequest::new(
//                     self.state.clone(),
//                     req,
//                     params,
//                 ))));
//             }
//         }
//         Err(req)
//     }
// }

pub struct RoutePatternBuilder<S> {
    pattern: String,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: PhantomData<S>,
}

impl<S> RoutePatternBuilder<S> {
    fn new(path: &str) -> RoutePatternBuilder<S> {
        RoutePatternBuilder {
            pattern: path.to_string(),
            methods: Vec::new(),
            headers: Vec::new(),
            state: PhantomData,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }

    pub fn map<T, U, F: IntoNewService<T>>(self, md: F) -> RouteBuilder<T, S, (), U>
    where
        T: NewService<
            Request = ServiceRequest<S>,
            Response = ServiceRequest<S, U>,
            InitError = (),
        >,
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
        self,
        handler: F,
    ) -> Route<
        impl NewService<
            Request = ServiceRequest<S>,
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
            headers: self.headers,
            state: PhantomData,
        }
    }

    pub fn with<F, P, R, I, E>(
        self,
        handler: F,
    ) -> Route<
        impl NewService<
            Request = ServiceRequest<S>,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: AsyncFactory<S, (), P, R, I, E>,
        P: FromRequest<S> + 'static,
        R: IntoFuture<Item = I, Error = E>,
        I: Into<Response>,
        E: Into<Error>,
    {
        Route {
            service: Extract::new(P::Config::default()).then(AsyncHandle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }
}

pub struct RouteBuilder<T, S, U1, U2> {
    service: T,
    pattern: String,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: PhantomData<(S, U1, U2)>,
}

impl<T, S, U1, U2> RouteBuilder<T, S, U1, U2>
where
    T: NewService<
        Request = ServiceRequest<S, U1>,
        Response = ServiceRequest<S, U2>,
        Error = Error,
        InitError = (),
    >,
{
    pub fn new<F: IntoNewService<T>>(path: &str, factory: F) -> Self {
        RouteBuilder {
            service: factory.into_new_service(),
            pattern: path.to_string(),
            methods: Vec::new(),
            headers: Vec::new(),
            state: PhantomData,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }

    pub fn map<T1, U3, F: IntoNewService<T1>>(
        self,
        md: F,
    ) -> RouteBuilder<
        impl NewService<
            Request = ServiceRequest<S, U1>,
            Response = ServiceRequest<S, U3>,
            Error = Error,
            InitError = (),
        >,
        S,
        U1,
        U2,
    >
    where
        T1: NewService<
            Request = ServiceRequest<S, U2>,
            Response = ServiceRequest<S, U3>,
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
            headers: self.headers,
            state: PhantomData,
        }
    }

    pub fn with<F, P, R, I, E>(
        self,
        handler: F,
    ) -> Route<
        impl NewService<
            Request = ServiceRequest<S, U1>,
            Response = Response,
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: AsyncFactory<S, U2, P, R, I, E>,
        P: FromRequest<S> + 'static,
        R: IntoFuture<Item = I, Error = E>,
        I: Into<Response>,
        E: Into<Error>,
    {
        Route {
            service: self
                .service
                .and_then(Extract::new(P::Config::default()))
                .then(AsyncHandle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }

    pub fn finish<F, P, R>(
        self,
        handler: F,
    ) -> Route<
        impl NewService<
            Request = ServiceRequest<S, U1>,
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
            headers: self.headers,
            state: PhantomData,
        }
    }
}
