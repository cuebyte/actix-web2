mod app;
mod extractor;
pub mod handler;
mod helpers;
mod request;
mod responder;
mod route;
pub mod test;
mod url;

mod framed_app;
mod framed_handler;
mod framed_route;

// re-export for convinience
pub use actix_http::{http, Error, HttpMessage, Response, ResponseError};

pub use crate::app::{App, AppService, State};
pub use crate::extractor::{Form, Json, Path, Query};
pub use crate::handler::FromRequest;
pub use crate::request::Request;
pub use crate::responder::{Either, Responder};
pub use crate::route::Route;

pub mod dev {
    pub use crate::handler::{AsyncFactory, Extract, Factory, Handle};
}

pub mod framed {
    pub use super::framed_app::{FramedApp, FramedAppService};
    pub use super::framed_handler::{
        FramedError, FramedExtract, FramedFactory, FramedHandle, FramedRequest,
    };
    pub use super::framed_route::FramedRoute;
}
