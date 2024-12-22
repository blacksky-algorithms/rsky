#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

pub static APP_USER_AGENT: &str = concat!(
    env!("CARGO_PKG_HOMEPAGE"),
    "@",
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
);

pub mod car;
pub mod firehose;
