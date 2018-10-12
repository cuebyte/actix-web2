# Actix web2 [![Build Status](https://travis-ci.org/actix/actix-web2.svg?branch=master)](https://travis-ci.org/actix/actix-web2) [![Build status](https://ci.appveyor.com/api/projects/status/kkdb4yce7qhm5w85/branch/master?svg=true)](https://ci.appveyor.com/project/fafhrd91/actix-web-hdy9d/branch/master) [![codecov](https://codecov.io/gh/actix/actix-web/branch/master/graph/badge.svg)](https://codecov.io/gh/actix/actix-web) [![crates.io](https://meritbadge.herokuapp.com/actix-web)](https://crates.io/crates/actix-web) [![Join the chat at https://gitter.im/actix/actix](https://badges.gitter.im/actix/actix.svg)](https://gitter.im/actix/actix?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge&utm_content=badge)

Actix web2 is a simple, pragmatic and extremely fast web framework for Rust.

## Documentation & community resources

* [User Guide](https://actix.rs/docs/)
* [API Documentation (Development)](https://actix.rs/actix-web2/actix_web2/)
* [API Documentation (Releases)](https://actix.rs/api/actix-web2/stable/actix_web2/)
* [Chat on gitter](https://gitter.im/actix/actix)
* Cargo package: [actix-web](https://crates.io/crates/actix-web2)
* Minimum supported Rust version: 1.26 or later

## Example

```rust
extern crate actix_web2;
use actix_web2::{Path, Responder};

fn index(info: Path<(u32, String)>) -> impl Responder {
    format!("Hello {}! id:{}", info.1, info.0)
}

fn main() {
    server::new(
        || App::new()
            .route("/{id}/{name}/index.html", http::Method::GET, index))
        .bind("127.0.0.1:8080").unwrap()
        .run();
}
```

## License

This project is licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))
* MIT license ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))

at your option.

## Code of Conduct

Contribution to the actix-web2 crate is organized under the terms of the
Contributor Covenant, the maintainer of actix-web2, @fafhrd91, promises to
intervene to uphold that code of conduct.
