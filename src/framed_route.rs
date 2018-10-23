use std::marker::PhantomData;

use futures::{Async, Future, IntoFuture, Poll};

use actix_http::h1::Codec;
use actix_http::http::{HeaderName, HeaderValue, Method};
use actix_http::{Error, Request};
use actix_net::codec::Framed;
use actix_net::service::{IntoNewService, NewService, NewServiceExt, Service};

use super::app::{HttpService, HttpServiceFactory, State};
use super::handler::FromRequest;
use super::param::Params;
use super::pattern::ResourcePattern;
use super::request::Request as WebRequest;

use super::framed_handler::{FramedExtract, FramedFactory, FramedHandle, FramedRequest};

/// Resource route definition
///
/// Route uses builder-like pattern for configuration.
/// If handler is not explicitly set, default *404 Not Found* handler is used.
pub struct FramedRoute<Io, T, S = ()> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: PhantomData<(S, Io)>,
}

impl<Io, S> FramedRoute<Io, (), S> {
    pub fn build(path: &str) -> FramedRoutePatternBuilder<Io, S> {
        FramedRoutePatternBuilder::new(path)
    }

    pub fn get(path: &str) -> FramedRoutePatternBuilder<Io, S> {
        FramedRoutePatternBuilder::new(path).method(Method::GET)
    }

    pub fn post(path: &str) -> FramedRoutePatternBuilder<Io, S> {
        FramedRoutePatternBuilder::new(path).method(Method::POST)
    }

    pub fn put(path: &str) -> FramedRoutePatternBuilder<Io, S> {
        FramedRoutePatternBuilder::new(path).method(Method::PUT)
    }

    pub fn delete(path: &str) -> FramedRoutePatternBuilder<Io, S> {
        FramedRoutePatternBuilder::new(path).method(Method::DELETE)
    }
}

impl<Io, T, S> FramedRoute<Io, T, S>
where
    T: NewService<Request = FramedRequest<Io, S>, Response = ()> + 'static,
{
    pub fn new<F: IntoNewService<T>>(pattern: ResourcePattern, factory: F) -> Self {
        FramedRoute {
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

impl<Io, T, S> HttpServiceFactory<S> for FramedRoute<Io, T, S>
where
    T: NewService<Request = FramedRequest<S, Framed<Io, Codec>>, Response = ()>
        + 'static,
{
    type Factory = FramedRouteFactory<Io, T, S>;

    fn create(self, state: State<S>) -> Self::Factory {
        FramedRouteFactory {
            state,
            service: self.service,
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            _t: PhantomData,
        }
    }
}

pub struct FramedRouteFactory<Io, T, S> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
    _t: PhantomData<Io>,
}

impl<Io, T, S> NewService for FramedRouteFactory<Io, T, S>
where
    T: NewService<Request = FramedRequest<S, Framed<Io, Codec>>, Response = ()>
        + 'static,
{
    type Request = (Request, Framed<Io, Codec>);
    type Response = T::Response;
    type Error = T::Error;
    type InitError = T::InitError;
    type Service = FramedRouteService<Io, T::Service, S>;
    type Future = CreateRouteService<Io, T, S>;

    fn new_service(&self) -> Self::Future {
        CreateRouteService {
            fut: self.service.new_service(),
            pattern: self.pattern.clone(),
            methods: self.methods.clone(),
            headers: self.headers.clone(),
            state: self.state.clone(),
            _t: PhantomData,
        }
    }
}

pub struct CreateRouteService<Io, T: NewService, S> {
    fut: T::Future,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
    _t: PhantomData<Io>,
}

impl<Io, T, S> Future for CreateRouteService<Io, T, S>
where
    T: NewService<Request = FramedRequest<S, Framed<Io, Codec>>, Response = ()>,
{
    type Item = FramedRouteService<Io, T::Service, S>;
    type Error = T::InitError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let service = try_ready!(self.fut.poll());

        Ok(Async::Ready(FramedRouteService {
            service,
            state: self.state.clone(),
            pattern: self.pattern.clone(),
            methods: self.methods.clone(),
            headers: self.headers.clone(),
            _t: PhantomData,
        }))
    }
}

pub struct FramedRouteService<Io, T, S> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: State<S>,
    _t: PhantomData<Io>,
}

impl<Io, T, S> Service for FramedRouteService<Io, T, S>
where
    T: Service<Request = FramedRequest<S, Framed<Io, Codec>>, Response = ()> + 'static,
{
    type Request = (Request, Framed<Io, Codec>);
    type Response = ();
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, (req, framed): Self::Request) -> Self::Future {
        self.service.call(FramedRequest::new(
            WebRequest::new(self.state.clone(), req, Params::new()),
            framed,
        ))
    }
}

impl<Io, T, S> HttpService for FramedRouteService<Io, T, S>
where
    Io: 'static,
    S: 'static,
    T: Service<Request = FramedRequest<S, Framed<Io, Codec>>, Response = ()> + 'static,
{
    fn handle(
        &mut self, (req, framed): Self::Request,
    ) -> Result<Self::Future, Self::Request> {
        if self.methods.is_empty()
            || !self.methods.is_empty() && self.methods.contains(req.method())
        {
            if let Some(params) = self.pattern.match_with_params(&req, 0) {
                return Ok(self.service.call(FramedRequest::new(
                    WebRequest::new(self.state.clone(), req, params),
                    framed,
                )));
            }
        }
        Err((req, framed))
    }
}

pub struct FramedRoutePatternBuilder<Io, S> {
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: PhantomData<(Io, S)>,
}

impl<Io, S> FramedRoutePatternBuilder<Io, S> {
    fn new(path: &str) -> FramedRoutePatternBuilder<Io, S> {
        FramedRoutePatternBuilder {
            pattern: ResourcePattern::new(path),
            methods: Vec::new(),
            headers: Vec::new(),
            state: PhantomData,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }

    pub fn map<T, U, F: IntoNewService<T>>(
        self, md: F,
    ) -> FramedRouteBuilder<Io, S, T, Framed<Io, Codec>, U>
    where
        T: NewService<
            Request = FramedRequest<S, Framed<Io, Codec>>,
            Response = FramedRequest<S, U>,
            InitError = (),
        >,
        T::Error: Into<Error>,
    {
        FramedRouteBuilder {
            service: md.into_new_service(),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }

    pub fn with<F, P, R, E>(
        self, handler: F,
    ) -> FramedRoute<
        Io,
        impl NewService<
            Request = FramedRequest<S, Framed<Io, Codec>>,
            Response = (),
            InitError = (),
        >,
        S,
    >
    where
        F: FramedFactory<S, P, Framed<Io, Codec>, R, E>,
        P: FromRequest<S> + 'static,
        R: IntoFuture<Item = (), Error = E>,
        E: Into<Error>,
    {
        FramedRoute {
            service: FramedExtract::new(P::Config::default())
                .and_then(FramedHandle::new(handler)),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }
}

pub struct FramedRouteBuilder<Io, S, T, U1, U2> {
    service: T,
    pattern: ResourcePattern,
    methods: Vec<Method>,
    headers: Vec<(HeaderName, HeaderValue)>,
    state: PhantomData<(Io, S, U1, U2)>,
}

impl<Io, S, T, U1, U2> FramedRouteBuilder<Io, S, T, U1, U2>
where
    T: NewService<
        Request = FramedRequest<S, U1>,
        Response = FramedRequest<S, U2>,
        Error = Error,
        InitError = (),
    >,
{
    pub fn new<F: IntoNewService<T>>(path: &str, factory: F) -> Self {
        FramedRouteBuilder {
            service: factory.into_new_service(),
            pattern: ResourcePattern::new(path),
            methods: Vec::new(),
            headers: Vec::new(),
            state: PhantomData,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }

    pub fn map<K, U3, F: IntoNewService<K>>(
        self, md: F,
    ) -> FramedRouteBuilder<
        Io,
        S,
        impl NewService<
            Request = FramedRequest<S, U1>,
            Response = FramedRequest<S, U3>,
            Error = K::Error,
            InitError = (),
        >,
        U1,
        U3,
    >
    where
        K: NewService<
            Request = FramedRequest<S, U2>,
            Response = FramedRequest<S, U3>,
            InitError = (),
        >,
        K::Error: From<T::Error>,
    {
        FramedRouteBuilder {
            service: self.service.from_err().and_then(md.into_new_service()),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }

    pub fn with<F, P, R, E>(
        self, handler: F,
    ) -> FramedRoute<
        Io,
        impl NewService<
            Request = FramedRequest<S, U1>,
            Response = (),
            Error = Error,
            InitError = (),
        >,
        S,
    >
    where
        F: FramedFactory<S, P, U2, R, E>,
        P: FromRequest<S> + 'static,
        R: IntoFuture<Item = (), Error = E>,
        E: Into<Error>,
    {
        FramedRoute {
            service: self
                .service
                .and_then(FramedExtract::new(P::Config::default()))
                .and_then(FramedHandle::new(handler))
                .map_err(Error::from),
            pattern: self.pattern,
            methods: self.methods,
            headers: self.headers,
            state: PhantomData,
        }
    }
}
