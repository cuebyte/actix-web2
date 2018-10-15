extern crate actix_http;
extern crate actix_net;
extern crate bytes;
extern crate encoding;
#[macro_use]
extern crate futures;
extern crate mime;
extern crate regex;
#[macro_use]
extern crate serde;
extern crate serde_urlencoded;

mod app;
mod de;
mod extractor;
pub mod handler;
mod helpers;
mod param;
pub mod pattern;
mod request;
mod responder;
mod route;

// re-export for convinience
pub use actix_http::{Error, HttpMessage, Response, ResponseError};

pub use app::{App, AppService, State};
pub use extractor::{Form, Json, Path, Query};
pub use handler::FromRequest;
pub use request::Request;
pub use responder::{Either, Responder};
pub use route::Route;

pub mod dev {
    pub use handler::{AsyncFactory, Extract, Factory, Handle};
    pub use param::Params;
}
