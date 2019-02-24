use hashbrown::hash_map::HashMap;
use std::rc::Rc;
use futures::{Async, Future, Poll};

use actix_http::error::Result;
use actix_http::{Response, http::StatusCode};
use actix_service::{IntoNewTransform, Service, Transform};

use crate::service::{ServiceRequest, ServiceResponse};
use crate::middleware::MiddlewareFactory;
use crate::request::HttpRequest;

type ErrorHandler = Fn(&HttpRequest, &Response) -> Result<Response>;

/// `Middleware` for allowing custom handlers for responses.
///
/// You can use `ErrorHandlers::handler()` method  to register a custom error
/// handler for specific status code. You can modify existing response or
/// create completely new one.
///
/// ## Example
///
/// ```rust
/// # extern crate actix_web;
/// use actix_web::middleware::{ErrorHandlers, Response};
/// use actix_web::{http, App, ServiceRequest, Response, Result};
///
/// fn render_500<P>(_: &ServiceRequest<P>, resp: Response) -> Result<Response> {
///     let mut builder = resp.into_builder();
///     builder.header(http::header::CONTENT_TYPE, "application/json");
///     Ok(Response::Done(builder.into()))
/// }
///
/// fn main() {
///     let app = App::new()
///         .middleware(
///             ErrorHandlers::new()
///                 .handler(http::StatusCode::INTERNAL_SERVER_ERROR, render_500),
///         )
///         .resource("/test", |r| {
///             r.method(http::Method::GET).f(|_| Response::Ok());
///             r.method(http::Method::HEAD)
///                 .f(|_| Response::MethodNotAllowed());
///         })
///         .finish();
/// }
/// ```

#[derive(Clone)]
pub struct ErrorHandlers {
    handlers: HashMap<StatusCode, Rc<ErrorHandler>>,
}

impl Default for ErrorHandlers {
    fn default() -> Self {
        ErrorHandlers {
            handlers: HashMap::new(),
        }
    }
}

impl ErrorHandlers {
    /// Construct new `ErrorHandlers` instance
    pub fn new() -> Self {
        ErrorHandlers::default()
    }

    /// Register error handler for specified status code
    pub fn handler<F>(mut self, status: StatusCode, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &Response) -> Result<Response> + 'static,
    {
        self.handlers.insert(status, Rc::new(handler));
        self
    }
}

impl<S, P> IntoNewTransform<MiddlewareFactory<ErrorHandlers, S>, S>
    for ErrorHandlers
where
    S: Service<Request = ServiceRequest<P>, Response = ServiceResponse>,
    S::Future: 'static,
{
    fn into_new_transform(self) -> MiddlewareFactory<ErrorHandlers, S> {
        MiddlewareFactory::new(self)
    }
}

impl<S, P> Transform<S> for ErrorHandlers
where
    S: Service<Request = ServiceRequest<P>, Response = ServiceResponse>,
    S::Future: 'static,
{
    type Request = ServiceRequest<P>;
    type Response = ServiceResponse;
    type Error = S::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: ServiceRequest<P>, srv: &mut S) -> Self::Future {
        srv.call(req).map(|resp| {
            let result = if let Some(handler) = self.handlers.get(&resp.status()) {
                handler(resp.request(), resp.response())
            } else {
                Ok(resp.response())
            };
        });
        unimplemented!()
    }
}

// impl<S: 'static> Middleware<P> for ErrorHandlers<P> {
//     fn response(&self, req: &ServiceRequest<P>, resp: Response) -> Result<Response> {
//         if let Some(handler) = self.handlers.get(&resp.status()) {
//             handler(req, resp)
//         } else {
//             Ok(Response::Done(resp))
//         }
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use error::{Error, ErrorInternalServerError};
    use http::header::CONTENT_TYPE;
    use http::StatusCode;
    use httpmessage::HttpMessage;
    use middleware::Started;
    use test::{self, TestRequest};

    fn render_500<P>(_: &ServiceRequest<P>, resp: Response) -> Result<Response> {
        let mut builder = resp.into_builder();
        builder.header(CONTENT_TYPE, "0001");
        Ok(Response::Done(builder.into()))
    }

    #[test]
    fn test_handler() {
        let mw =
            ErrorHandlers::new().handler(StatusCode::INTERNAL_SERVER_ERROR, render_500);

        let mut req = TestRequest::default().finish();
        let resp = Response::InternalServerError().finish();
        let resp = match mw.response(&mut req, resp) {
            Ok(Response::Done(resp)) => resp,
            _ => panic!(),
        };
        assert_eq!(resp.headers().get(CONTENT_TYPE).unwrap(), "0001");

        let resp = Response::Ok().finish();
        let resp = match mw.response(&mut req, resp) {
            Ok(Response::Done(resp)) => resp,
            _ => panic!(),
        };
        assert!(!resp.headers().contains_key(CONTENT_TYPE));
    }

    struct MiddlewareOne;

    impl<P> Middleware<P> for MiddlewareOne {
        fn start(&self, _: &ServiceRequest<P>) -> Result<Started, Error> {
            Err(ErrorInternalServerError("middleware error"))
        }
    }

    #[test]
    fn test_middleware_start_error() {
        let mut srv = test::TestServer::new(move |app| {
            app.middleware(
                ErrorHandlers::new()
                    .handler(StatusCode::INTERNAL_SERVER_ERROR, render_500),
            ).middleware(MiddlewareOne)
            .handler(|_| Response::Ok())
        });

        let request = srv.get().finish().unwrap();
        let response = srv.execute(request.send()).unwrap();
        assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "0001");
    }
}
