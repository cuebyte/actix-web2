//! Various helpers for Actix applications to use during testing.
use std::str::FromStr;
use std::{net, sync::mpsc, thread};

use futures::{Future, IntoFuture};
use http::header::HeaderName;
use http::{HeaderMap, HttpTryFrom, Method, Uri, Version};
use net2::TcpBuilder;
use tokio::runtime::current_thread::Runtime;

use actix::{Actor, Addr, System};
use actix_net::server::{Server, StreamServiceFactory};
use actix_web::client::{ClientConnector, ClientRequest, ClientRequestBuilder};
use actix_web::ws;

use actix_http::dev::Payload;
use actix_http::http::header::{Header, IntoHeaderValue};
use actix_http::uri::Url as InnerUrl;
use actix_http::Binary;
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
    pub fn set_payload<B: Into<Binary>>(mut self, data: B) -> Self {
        let mut data = data.into();
        let mut payload = Payload::empty();
        payload.unread_data(data.take());
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

        let mut req = HttpRequest::new();
        {
            let inner = req.inner_mut();
            inner.method = method;
            inner.url = InnerUrl::new(uri);
            inner.version = version;
            inner.headers = headers;
            *inner.payload.borrow_mut() = payload;
        }
        params.set_url(req.url().clone());

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
pub struct TestServer {
    addr: net::SocketAddr,
    conn: Addr<ClientConnector>,
    rt: Runtime,
}

impl TestServer {
    /// Start new test server with application factory
    pub fn with_factory<F: StreamServiceFactory>(factory: F) -> Self {
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

            tx.send((System::current(), local_addr, TestServer::get_conn()))
                .unwrap();
            sys.run();
        });

        let (system, addr, conn) = rx.recv().unwrap();
        System::set_current(system);
        TestServer {
            addr,
            conn,
            rt: Runtime::new().unwrap(),
        }
    }

    fn get_conn() -> Addr<ClientConnector> {
        #[cfg(any(feature = "alpn", feature = "ssl"))]
        {
            use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};

            let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
            builder.set_verify(SslVerifyMode::NONE);
            ClientConnector::with_connector(builder.build()).start()
        }
        #[cfg(all(
            feature = "rust-tls",
            not(any(feature = "alpn", feature = "ssl"))
        ))]
        {
            use rustls::ClientConfig;
            use std::fs::File;
            use std::io::BufReader;
            let mut config = ClientConfig::new();
            let pem_file = &mut BufReader::new(File::open("tests/cert.pem").unwrap());
            config.root_store.add_pem_file(pem_file).unwrap();
            ClientConnector::with_connector(config).start()
        }
        #[cfg(not(any(feature = "alpn", feature = "ssl", feature = "rust-tls")))]
        {
            ClientConnector::default().start()
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

    /// Stop http server
    fn stop(&mut self) {
        System::current().stop();
    }

    /// Execute future on current core
    pub fn execute<F, I, E>(&mut self, fut: F) -> Result<I, E>
    where
        F: Future<Item = I, Error = E>,
    {
        self.rt.block_on(fut)
    }

    /// Connect to websocket server at a given path
    pub fn ws_at(
        &mut self, path: &str,
    ) -> Result<(ws::ClientReader, ws::ClientWriter), ws::ClientError> {
        let url = self.url(path);
        self.rt
            .block_on(ws::Client::with_connector(url, self.conn.clone()).connect())
    }

    /// Connect to a websocket server
    pub fn ws(
        &mut self,
    ) -> Result<(ws::ClientReader, ws::ClientWriter), ws::ClientError> {
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
            .with_connector(self.conn.clone())
            .take()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop()
    }
}
