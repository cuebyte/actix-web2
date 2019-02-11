use std::ops::Deref;
use std::rc::Rc;

use actix_http::Error;
use futures::future::{ok, FutureResult};
use futures::{Future, IntoFuture};

use crate::handler::FromRequest;
use crate::request::HttpRequest;

/// Application state
pub struct State<S>(Rc<S>);

impl<S> State<S> {
    pub fn new(state: S) -> State<S> {
        State(Rc::new(state))
    }

    pub fn get_ref(&self) -> &S {
        self.0.as_ref()
    }
}

impl<S> Deref for State<S> {
    type Target = S;

    fn deref(&self) -> &S {
        self.0.as_ref()
    }
}

impl<S> Clone for State<S> {
    fn clone(&self) -> State<S> {
        State(self.0.clone())
    }
}

impl<S> FromRequest<S> for State<S> {
    type Error = Error;
    type Future = FutureResult<Self, Error>;

    #[inline]
    fn from_request(req: &HttpRequest<S>) -> Self::Future {
        ok(req.get_state())
    }
}

/// Application state factory
pub trait StateFactory<S> {
    fn construct(&self) -> Box<Future<Item = S, Error = ()>>;
}

impl<F, Out> StateFactory<Out::Item> for F
where
    F: Fn() -> Out + 'static,
    Out: IntoFuture + 'static,
    Out::Error: std::fmt::Debug,
{
    fn construct(&self) -> Box<Future<Item = Out::Item, Error = ()>> {
        Box::new((*self)().into_future().map_err(|e| {
            log::error!("Can not construct application state: {:?}", e);
        }))
    }
}
