use std::ops::Deref;
use std::rc::Rc;

use futures::Future;
use smallvec::SmallVec;

use actix_http::http::Method;
use actix_http::{Error, Request, Response};

use handler::{AsyncResult, FromRequest, Handler, Responder};
use pattern::ResourcePattern;

/// *Resource* is an entry in route table which corresponds to requested URL.
///
/// Resource in turn has at least one route.
/// Route consists of an object that implements `Handler` trait (handler)
/// and list of predicates (objects that implement `Predicate` trait).
/// Route uses builder-like pattern for configuration.
/// During request handling, resource object iterate through all routes
/// and check all predicates for specific route, if request matches all
/// predicates route route considered matched and route handler get called.
pub struct Resource {
    rdef: ResourcePattern,
    routes: SmallVec<[Route<S>; 3]>,
}

impl Resource {
    /// Create new resource with specified resource definition
    pub fn new<S>(path: &str, service: S) -> Self {
        unimplemented!()
    }

    /// Create new resource with specified resource definition
    pub fn with_prefix<S>(path: &str, service: S) -> Self {
        unimplemented!()
    }

    /// Resource definition
    pub fn rdef(&self) -> &ResourceDef {
        &self.rdef
    }
}

impl Resource {}

impl Service for Resource {}

/// Default resource
pub struct DefaultResource(Rc<Resource>);

impl<S> Deref for DefaultResource<S> {
    type Target = Resource<S>;

    fn deref(&self) -> &Resource<S> {
        self.0.as_ref()
    }
}

impl<S> Clone for DefaultResource<S> {
    fn clone(&self) -> Self {
        DefaultResource(self.0.clone())
    }
}

impl<S> From<Resource<S>> for DefaultResource<S> {
    fn from(res: Resource<S>) -> Self {
        DefaultResource(Rc::new(res))
    }
}
