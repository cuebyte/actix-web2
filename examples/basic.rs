use futures::IntoFuture;

use actix_http::h1;
use actix_server::Server;
use actix_service::NewService;
use actix_web2::{App, Error, HttpRequest, Route};

fn index(req: HttpRequest) -> &'static str {
    println!("REQ: {:?}", req);
    "Hello world!\r\n"
}

fn index_async(req: HttpRequest) -> impl IntoFuture<Item = &'static str, Error = Error> {
    println!("REQ: {:?}", req);
    Ok("Hello world!\r\n")
}

fn no_params() -> &'static str {
    "Hello world!\r\n"
}

fn main() {
    ::std::env::set_var("RUST_LOG", "actix_server=info,actix_web2=info");
    env_logger::init();
    let sys = actix_rt::System::new("hello-world");

    Server::build()
        .bind("test", "127.0.0.1:8080", || {
            h1::H1Service::new(
                App::new()
                    .service(Route::build("/resource1/index.html").finish(index))
                    .service(Route::build("/resource2/index.html").with(index_async))
                    .service(Route::build("/test1.html").finish(|| "Test\r\n"))
                    .service(Route::build("/").finish(no_params)),
            )
            .map(|_| ())
        })
        .unwrap()
        .workers(1)
        .start();

    let _ = sys.run();
}
