//! Various helpers for Actix applications to use during testing.
use std::marker::PhantomData;
use std::str::FromStr;
use std::{net, sync::mpsc, thread};

use bytes::Bytes;
use futures::{Future, IntoFuture};
use http::header::HeaderName;
use http::{HeaderMap, HttpTryFrom, Method, Uri, Version};
use net2::TcpBuilder;
use tokio::runtime::current_thread::Runtime;
use tokio_io::{AsyncRead, AsyncWrite};

use actix::System;
use actix_net::codec::Framed;
use actix_net::server::{Server, StreamServiceFactory};
use actix_net::service::Service;

use actix_http::client::{
    ClientRequest, ClientRequestBuilder, Connect, Connection, Connector, ConnectorError,
};
use actix_http::dev::Payload;
use actix_http::http::header::{Header, IntoHeaderValue};
use actix_http::ws;
use actix_http::Request as HttpRequest;

use app::State;
use param::Params;
use request::Request;

/// Test `Request` builder
///
/// ```rust,ignore
/// # extern crate http;
/// # extern crate actix_web;
/// # use http::{header, StatusCode};
/// # use actix_web::*;
/// use actix_web::test::TestRequest;
///
/// fn index(req: &HttpRequest) -> HttpResponse {
///     if let Some(hdr) = req.headers().get(header::CONTENT_TYPE) {
///         HttpResponse::Ok().into()
///     } else {
///         HttpResponse::BadRequest().into()
///     }
/// }
///
/// fn main() {
///     let resp = TestRequest::with_header("content-type", "text/plain")
///         .run(&index)
///         .unwrap();
///     assert_eq!(resp.status(), StatusCode::OK);
///
///     let resp = TestRequest::default().run(&index).unwrap();
///     assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
/// }
/// ```
pub struct TestRequest<S> {
    state: S,
    version: Version,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    params: Params,
    payload: Option<Payload>,
}

impl Default for TestRequest<()> {
    fn default() -> TestRequest<()> {
        TestRequest {
            state: (),
            method: Method::GET,
            uri: Uri::from_str("/").unwrap(),
            version: Version::HTTP_11,
            headers: HeaderMap::new(),
            params: Params::new(),
            payload: None,
        }
    }
}

impl TestRequest<()> {
    /// Create TestRequest and set request uri
    pub fn with_uri(path: &str) -> TestRequest<()> {
        TestRequest::default().uri(path)
    }

    /// Create TestRequest and set header
    pub fn with_hdr<H: Header>(hdr: H) -> TestRequest<()> {
        TestRequest::default().set(hdr)
    }

    /// Create TestRequest and set header
    pub fn with_header<K, V>(key: K, value: V) -> TestRequest<()>
    where
        HeaderName: HttpTryFrom<K>,
        V: IntoHeaderValue,
    {
        TestRequest::default().header(key, value)
    }
}

impl<S: 'static> TestRequest<S> {
    /// Start HttpRequest build process with application state
    pub fn with_state(state: S) -> TestRequest<S> {
        TestRequest {
            state,
            method: Method::GET,
            uri: Uri::from_str("/").unwrap(),
            version: Version::HTTP_11,
            headers: HeaderMap::new(),
            params: Params::new(),
            payload: None,
        }
    }

    /// Set HTTP version of this request
    pub fn version(mut self, ver: Version) -> Self {
        self.version = ver;
        self
    }

    /// Set HTTP method of this request
    pub fn method(mut self, meth: Method) -> Self {
        self.method = meth;
        self
    }

    /// Set HTTP Uri of this request
    pub fn uri(mut self, path: &str) -> Self {
        self.uri = Uri::from_str(path).unwrap();
        self
    }

    /// Set a header
    pub fn set<H: Header>(mut self, hdr: H) -> Self {
        if let Ok(value) = hdr.try_into() {
            self.headers.append(H::name(), value);
            return self;
        }
        panic!("Can not set header");
    }

    /// Set a header
    pub fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        HeaderName: HttpTryFrom<K>,
        V: IntoHeaderValue,
    {
        if let Ok(key) = HeaderName::try_from(key) {
            if let Ok(value) = value.try_into() {
                self.headers.append(key, value);
                return self;
            }
        }
        panic!("Can not create header");
    }

    /// Set request path pattern parameter
    pub fn param(mut self, name: &'static str, value: &'static str) -> Self {
        self.params.add_static(name, value);
        self
    }

    /// Set request payload
    pub fn set_payload<B: Into<Bytes>>(mut self, data: B) -> Self {
        let mut payload = Payload::empty();
        payload.unread_data(data.into());
        self.payload = Some(payload);
        self
    }

    /// Complete request creation and generate `HttpRequest` instance
    pub fn finish(self) -> Request<S> {
        let TestRequest {
            state,
            method,
            uri,
            version,
            headers,
            mut params,
            payload,
        } = self;

        params.set_uri(&uri);

        let mut req = HttpRequest::new();
        {
            let inner = req.inner_mut();
            inner.head.uri = uri;
            inner.head.method = method;
            inner.head.version = version;
            inner.head.headers = headers;
            *inner.payload.borrow_mut() = payload;
        }

        Request::new(State::new(state), req, params)
    }

    /// This method generates `HttpRequest` instance and executes handler
    pub fn run_async<F, R, I, E>(self, f: F) -> Result<I, E>
    where
        F: FnOnce(&Request<S>) -> R,
        R: IntoFuture<Item = I, Error = E>,
    {
        let mut rt = Runtime::new().unwrap();
        rt.block_on(f(&self.finish()).into_future())
    }
}

/// The `TestServer` type.
///
/// `TestServer` is very simple test server that simplify process of writing
/// integration tests cases for actix web applications.
///
/// # Examples
///
/// ```rust
/// # extern crate actix_web;
/// # use actix_web::*;
/// #
/// # fn my_handler(req: &HttpRequest) -> HttpResponse {
/// #     HttpResponse::Ok().into()
/// # }
/// #
/// # fn main() {
/// use actix_web::test::TestServer;
///
/// let mut srv = TestServer::new(|app| app.handler(my_handler));
///
/// let req = srv.get().finish().unwrap();
/// let response = srv.execute(req.send()).unwrap();
/// assert!(response.status().is_success());
/// # }
/// ```
pub struct TestServer<T = (), C = ()> {
    addr: net::SocketAddr,
    conn: T,
    rt: Runtime,
    _t: PhantomData<C>,
}

impl TestServer<(), ()> {
    /// Start new test server with application factory
    pub fn with_factory<F: StreamServiceFactory>(
        factory: F,
    ) -> TestServer<
        impl Service<Request = Connect, Response = impl Connection, Error = ConnectorError>
            + Clone,
    > {
        let (tx, rx) = mpsc::channel();

        // run server in separate thread
        thread::spawn(move || {
            let sys = System::new("actix-test-server");
            let tcp = net::TcpListener::bind("127.0.0.1:0").unwrap();
            let local_addr = tcp.local_addr().unwrap();

            Server::default()
                .listen("test", tcp, factory)
                .workers(1)
                .disable_signals()
                .start();

            tx.send((System::current(), local_addr)).unwrap();
            sys.run();
        });

        let (system, addr) = rx.recv().unwrap();
        System::set_current(system);
        TestServer {
            addr,
            conn: TestServer::new_connector(),
            rt: Runtime::new().unwrap(),
            _t: PhantomData,
        }
    }

    fn new_connector(
) -> impl Service<Request = Connect, Response = impl Connection, Error = ConnectorError>
             + Clone {
        #[cfg(feature = "ssl")]
        {
            use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};

            let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
            builder.set_verify(SslVerifyMode::NONE);
            Connector::default().ssl(builder.build()).service()
        }
        #[cfg(not(feature = "ssl"))]
        {
            Connector::default().service()
        }
    }

    /// Get firat available unused address
    pub fn unused_addr() -> net::SocketAddr {
        let addr: net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let socket = TcpBuilder::new_v4().unwrap();
        socket.bind(&addr).unwrap();
        socket.reuse_address(true).unwrap();
        let tcp = socket.to_tcp_listener().unwrap();
        tcp.local_addr().unwrap()
    }
}

impl<T, C> TestServer<T, C> {
    /// Execute future on current core
    pub fn block_on<F, I, E>(&mut self, fut: F) -> Result<I, E>
    where
        F: Future<Item = I, Error = E>,
    {
        self.rt.block_on(fut)
    }

    /// Construct test server url
    pub fn addr(&self) -> net::SocketAddr {
        self.addr
    }

    /// Construct test server url
    pub fn url(&self, uri: &str) -> String {
        if uri.starts_with('/') {
            format!("http://localhost:{}{}", self.addr.port(), uri)
        } else {
            format!("http://localhost:{}/{}", self.addr.port(), uri)
        }
    }

    /// Http connector
    pub fn connector(&mut self) -> &mut T {
        &mut self.conn
    }

    /// Stop http server
    fn stop(&mut self) {
        System::current().stop();
    }
}

impl<T, C> TestServer<T, C>
where
    T: Service<Request = Connect, Response = C, Error = ConnectorError> + Clone,
    C: Connection,
{
    /// Connect to websocket server at a given path
    pub fn ws_at(
        &mut self,
        path: &str,
    ) -> Result<Framed<impl AsyncRead + AsyncWrite, ws::Codec>, ws::ClientError> {
        let url = self.url(path);
        self.rt
            .block_on(ws::Client::default().call(ws::Connect::new(url)))
    }

    /// Connect to a websocket server
    pub fn ws(
        &mut self,
    ) -> Result<Framed<impl AsyncRead + AsyncWrite, ws::Codec>, ws::ClientError> {
        self.ws_at("/")
    }

    /// Create `GET` request
    pub fn get(&self) -> ClientRequestBuilder {
        ClientRequest::get(self.url("/").as_str())
    }

    /// Create `POST` request
    pub fn post(&self) -> ClientRequestBuilder {
        ClientRequest::post(self.url("/").as_str())
    }

    /// Create `HEAD` request
    pub fn head(&self) -> ClientRequestBuilder {
        ClientRequest::head(self.url("/").as_str())
    }

    /// Connect to test http server
    pub fn client(&self, meth: Method, path: &str) -> ClientRequestBuilder {
        ClientRequest::build()
            .method(meth)
            .uri(self.url(path).as_str())
            .take()
    }
}

impl<T, C> Drop for TestServer<T, C> {
    fn drop(&mut self) {
        self.stop()
    }
}
