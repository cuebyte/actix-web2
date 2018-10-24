extern crate actix;
extern crate actix_http;
extern crate actix_net;
extern crate actix_web;
extern crate bytes;
extern crate encoding;
#[macro_use]
extern crate futures;
extern crate mime;
extern crate net2;
extern crate regex;
#[macro_use]
extern crate serde;
extern crate serde_urlencoded;
extern crate tokio;
extern crate tokio_io;

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
pub mod test;

mod framed_app;
mod framed_handler;
mod framed_route;

// re-export for convinience
pub use actix_http::{http, Error, HttpMessage, Response, ResponseError};

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

pub mod framed {
    pub use super::framed_app::{FramedApp, FramedAppService};
    pub use super::framed_handler::{
        FramedExtract, FramedFactory, FramedHandle, FramedRequest,
    };
    pub use super::framed_route::FramedRoute;
}
