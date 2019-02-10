//! Route match predicates
#![allow(non_snake_case)]
use std::marker::PhantomData;

use actix_http::http::{self, header, HttpTryFrom};

use crate::service::ServiceRequest;

/// Trait defines resource predicate.
/// Predicate can modify request object. It is also possible to
/// to store extra attributes on request by using `Extensions` container,
/// Extensions container available via `HttpRequest::extensions()` method.
pub trait Filter<S> {
    /// Check if request matches predicate
    fn check(&self, request: &mut ServiceRequest<S>) -> bool;
}

/// Return filter that matches if any of supplied filters.
///
/// ```rust
/// # extern crate actix_web;
/// use actix_web2::{filter, App, HttpResponse};
///
/// fn main() {
///     App::new().resource("/index.html", |r| {
///         r.route()
///             .filter(pred::Any(pred::Get()).or(pred::Post()))
///             .f(|r| HttpResponse::MethodNotAllowed())
///     });
/// }
/// ```
pub fn Any<S, F: Filter<S> + 'static>(filter: F) -> AnyFilter<S> {
    AnyFilter(vec![Box::new(filter)])
}

/// Matches if any of supplied filters matche.
pub struct AnyFilter<S>(Vec<Box<Filter<S>>>);

impl<S> AnyFilter<S> {
    /// Add filter to a list of filters to check
    pub fn or<F: Filter<S> + 'static>(mut self, filter: F) -> Self {
        self.0.push(Box::new(filter));
        self
    }
}

impl<S> Filter<S> for AnyFilter<S> {
    fn check(&self, req: &mut ServiceRequest<S>) -> bool {
        for p in &self.0 {
            if p.check(req) {
                return true;
            }
        }
        false
    }
}

/// Return filter that matches if all of supplied filters match.
///
/// ```rust
/// # extern crate actix_web;
/// use actix_web::{pred, App, HttpResponse};
///
/// fn main() {
///     App::new().resource("/index.html", |r| {
///         r.route()
///             .filter(
///                 pred::All(pred::Get())
///                     .and(pred::Header("content-type", "text/plain")),
///             )
///             .f(|_| HttpResponse::MethodNotAllowed())
///     });
/// }
/// ```
pub fn All<S, F: Filter<S> + 'static>(filter: F) -> AllFilter<S> {
    AllFilter(vec![Box::new(filter)])
}

/// Matches if all of supplied filters matche.
pub struct AllFilter<S>(Vec<Box<Filter<S>>>);

impl<S> AllFilter<S> {
    /// Add new predicate to list of predicates to check
    pub fn and<F: Filter<S> + 'static>(mut self, filter: F) -> Self {
        self.0.push(Box::new(filter));
        self
    }
}

impl<S> Filter<S> for AllFilter<S> {
    fn check(&self, request: &mut ServiceRequest<S>) -> bool {
        for p in &self.0 {
            if !p.check(request) {
                return false;
            }
        }
        true
    }
}

/// Return predicate that matches if supplied predicate does not match.
pub fn Not<S, F: Filter<S> + 'static>(filter: F) -> NotFilter<S> {
    NotFilter(Box::new(filter))
}

#[doc(hidden)]
pub struct NotFilter<S>(Box<Filter<S>>);

impl<S> Filter<S> for NotFilter<S> {
    fn check(&self, request: &mut ServiceRequest<S>) -> bool {
        !self.0.check(request)
    }
}

/// Http method predicate
#[doc(hidden)]
pub struct MethodFilter<S>(http::Method, PhantomData<S>);

impl<S> Filter<S> for MethodFilter<S> {
    fn check(&self, request: &mut ServiceRequest<S>) -> bool {
        request.method() == self.0
    }
}

/// Predicate to match *GET* http method
pub fn Get<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::GET, PhantomData)
}

/// Predicate to match *POST* http method
pub fn Post<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::POST, PhantomData)
}

/// Predicate to match *PUT* http method
pub fn Put<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::PUT, PhantomData)
}

/// Predicate to match *DELETE* http method
pub fn Delete<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::DELETE, PhantomData)
}

/// Predicate to match *HEAD* http method
pub fn Head<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::HEAD, PhantomData)
}

/// Predicate to match *OPTIONS* http method
pub fn Options<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::OPTIONS, PhantomData)
}

/// Predicate to match *CONNECT* http method
pub fn Connect<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::CONNECT, PhantomData)
}

/// Predicate to match *PATCH* http method
pub fn Patch<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::PATCH, PhantomData)
}

/// Predicate to match *TRACE* http method
pub fn Trace<S>() -> MethodFilter<S> {
    MethodFilter(http::Method::TRACE, PhantomData)
}

/// Predicate to match specified http method
pub fn Method<S>(method: http::Method) -> MethodFilter<S> {
    MethodFilter(method, PhantomData)
}

/// Return predicate that matches if request contains specified header and
/// value.
pub fn Header<S>(name: &'static str, value: &'static str) -> HeaderFilter<S> {
    HeaderFilter(
        header::HeaderName::try_from(name).unwrap(),
        header::HeaderValue::from_static(value),
        PhantomData,
    )
}

#[doc(hidden)]
pub struct HeaderFilter<S>(header::HeaderName, header::HeaderValue, PhantomData<S>);

impl<S> Filter<S> for HeaderFilter<S> {
    fn check(&self, req: &mut ServiceRequest<S>) -> bool {
        if let Some(val) = req.headers().get(&self.0) {
            return val == self.1;
        }
        false
    }
}

/// Return predicate that matches if request contains specified Host name.
///
/// ```rust
/// # extern crate actix_web;
/// use actix_web::{pred, App, HttpResponse};
///
/// fn main() {
///     App::new().resource("/index.html", |r| {
///         r.route()
///             .filter(pred::Host("www.rust-lang.org"))
///             .f(|_| HttpResponse::MethodNotAllowed())
///     });
/// }
/// ```
pub fn Host<S: 'static, H: AsRef<str>>(host: H) -> HostFilter<S> {
    HostFilter(host.as_ref().to_string(), None, PhantomData)
}

#[doc(hidden)]
pub struct HostFilter<S>(String, Option<String>, PhantomData<S>);

impl<S> HostFilter<S> {
    /// Set reuest scheme to match
    pub fn scheme<H: AsRef<str>>(&mut self, scheme: H) {
        self.1 = Some(scheme.as_ref().to_string())
    }
}

impl<S: 'static> Filter<S> for HostFilter<S> {
    fn check(&self, _req: &mut ServiceRequest<S>) -> bool {
        // let info = req.connection_info();
        // if let Some(ref scheme) = self.1 {
        //     self.0 == info.host() && scheme == info.scheme()
        // } else {
        //     self.0 == info.host()
        // }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{header, Method};
    use test::TestRequest;

    #[test]
    fn test_header() {
        let req = TestRequest::with_header(
            header::TRANSFER_ENCODING,
            header::HeaderValue::from_static("chunked"),
        )
        .finish();

        let pred = Header("transfer-encoding", "chunked");
        assert!(pred.check(&req, req.state()));

        let pred = Header("transfer-encoding", "other");
        assert!(!pred.check(&req, req.state()));

        let pred = Header("content-type", "other");
        assert!(!pred.check(&req, req.state()));
    }

    #[test]
    fn test_host() {
        let req = TestRequest::default()
            .header(
                header::HOST,
                header::HeaderValue::from_static("www.rust-lang.org"),
            )
            .finish();

        let pred = Host("www.rust-lang.org");
        assert!(pred.check(&req, req.state()));

        let pred = Host("localhost");
        assert!(!pred.check(&req, req.state()));
    }

    #[test]
    fn test_methods() {
        let req = TestRequest::default().finish();
        let req2 = TestRequest::default().method(Method::POST).finish();

        assert!(Get().check(&req, req.state()));
        assert!(!Get().check(&req2, req2.state()));
        assert!(Post().check(&req2, req2.state()));
        assert!(!Post().check(&req, req.state()));

        let r = TestRequest::default().method(Method::PUT).finish();
        assert!(Put().check(&r, r.state()));
        assert!(!Put().check(&req, req.state()));

        let r = TestRequest::default().method(Method::DELETE).finish();
        assert!(Delete().check(&r, r.state()));
        assert!(!Delete().check(&req, req.state()));

        let r = TestRequest::default().method(Method::HEAD).finish();
        assert!(Head().check(&r, r.state()));
        assert!(!Head().check(&req, req.state()));

        let r = TestRequest::default().method(Method::OPTIONS).finish();
        assert!(Options().check(&r, r.state()));
        assert!(!Options().check(&req, req.state()));

        let r = TestRequest::default().method(Method::CONNECT).finish();
        assert!(Connect().check(&r, r.state()));
        assert!(!Connect().check(&req, req.state()));

        let r = TestRequest::default().method(Method::PATCH).finish();
        assert!(Patch().check(&r, r.state()));
        assert!(!Patch().check(&req, req.state()));

        let r = TestRequest::default().method(Method::TRACE).finish();
        assert!(Trace().check(&r, r.state()));
        assert!(!Trace().check(&req, req.state()));
    }

    #[test]
    fn test_preds() {
        let r = TestRequest::default().method(Method::TRACE).finish();

        assert!(Not(Get()).check(&r, r.state()));
        assert!(!Not(Trace()).check(&r, r.state()));

        assert!(All(Trace()).and(Trace()).check(&r, r.state()));
        assert!(!All(Get()).and(Trace()).check(&r, r.state()));

        assert!(Any(Get()).or(Trace()).check(&r, r.state()));
        assert!(!Any(Get()).or(Get()).check(&r, r.state()));
    }
}
