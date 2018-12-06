extern crate actix;
extern crate actix_http;
extern crate actix_net;
extern crate actix_web2;
extern crate env_logger;
extern crate futures;

use futures::IntoFuture;

use actix_http::h1;
use actix_net::server::Server;
use actix_net::service::NewServiceExt;
use actix_web2::{App, Error, Request, Route};

fn index(req: Request) -> &'static str {
    println!("REQ: {:?}", req);
    "Hello world!\r\n"
}

fn index_async(req: Request) -> impl IntoFuture<Item = &'static str, Error = Error> {
    println!("REQ: {:?}", req);
    Ok("Hello world!\r\n")
}

fn no_params() -> &'static str {
    "Hello world!\r\n"
}

fn main() {
    ::std::env::set_var("RUST_LOG", "actix_net=info,actix_web2=info");
    env_logger::init();
    let sys = actix::System::new("hello-world");

    Server::default()
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
        .start();

    let _ = sys.run();
}
