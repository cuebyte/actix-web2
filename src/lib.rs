#![allow(dead_code)]
mod app;
mod extractor;
pub mod handler;
mod helpers;
mod request;
mod responder;
mod route;
mod state;

// re-export for convenience
pub use actix_http::{http, Error, HttpMessage, Response, ResponseError};

pub use crate::app::{App, AppService};
pub use crate::extractor::{Form, Json, Path, Query};
pub use crate::handler::FromRequest;
pub use crate::request::Request;
pub use crate::responder::{Either, Responder};
pub use crate::route::Route;
pub use crate::state::State;

pub mod dev {
    pub use crate::handler::{AsyncFactory, Extract, Factory, Handle};
}
