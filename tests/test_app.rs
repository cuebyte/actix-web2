use actix_http::{h1, test::TestServer, ResponseError};
use actix_service::NewService;
use actix_web2::{http, App, Error, Request, Response, Route};
use derive_more::Display;

#[derive(Debug, Display)]
struct TestError;

impl ResponseError for TestError {
    fn error_response(&self) -> Response {
        Response::new(http::StatusCode::BAD_REQUEST)
    }
}

#[test]
fn test_error() {
    let mut srv = TestServer::with_factory(move || {
        h1::H1Service::build()
            .finish(
                App::new().service(
                    Route::post("/test-error")
                        .with(|_: Request| Err::<Response, Error>(TestError.into())),
                ),
            )
            .map(|_| ())
            .map_err(|_| ())
    });
    let mut connector = srv.new_connector();

    let request = srv
        .client(http::Method::POST, "/test-error")
        .finish()
        .unwrap();
    let response = srv.block_on(request.send(&mut connector)).unwrap();
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
}
