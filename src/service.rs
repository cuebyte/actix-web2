use actix_http::{Request, Response};
use actix_router::{Path, Url};

use crate::request::HttpRequest;
use crate::state::State;

pub struct ServiceRequest<S>(State<S>, Path<Url>, Request);

impl<S> ServiceRequest<S> {
    pub(crate) fn new(state: State<S>, path: Path<Url>, req: Request) -> Self {
        ServiceRequest(state, path, req)
    }

    pub fn into_request(self) -> HttpRequest<S> {
        HttpRequest::new(self.0, self.1, self.2)
    }

    pub fn into_response(self) -> ServiceResponse {
        ServiceResponse::Unhandled(self.2)
    }
}

#[derive(From)]
/// Http service response type
pub enum ServiceResponse {
    Response(Response),
    Unhandled(Request),
}

impl Into<Response> for ServiceResponse {
    fn into(self) -> Response {
        match self {
            ServiceResponse::Response(res) => res,
            ServiceResponse::Unhandled(_) => Response::NotFound().finish(),
        }
    }
}
