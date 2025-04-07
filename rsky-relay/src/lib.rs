#![deny(
    deprecated_safe,
    future_incompatible,
    let_underscore,
    keyword_idents,
    nonstandard_style,
    refining_impl_trait,
    rust_2018_compatibility,
    rust_2018_idioms,
    rust_2021_compatibility,
    rust_2024_compatibility,
    unused,
    warnings,
    clippy::all,
    clippy::cargo,
    clippy::dbg_macro,
    clippy::expect_used,
    clippy::iter_over_hash_type,
    clippy::nursery,
    clippy::pathbuf_init_then_push,
    clippy::pedantic,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::renamed_function_params,
    clippy::str_to_string,
    clippy::string_to_string,
    clippy::unused_result_ok,
    clippy::unwrap_used
)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc, clippy::missing_safety_doc)]

mod crawler;
mod publisher;
mod server;
mod types;
mod validator;

use std::sync::atomic::AtomicBool;

pub static SHUTDOWN: AtomicBool = AtomicBool::new(false);

pub use crawler::Manager as CrawlerManager;
pub use publisher::Manager as PublisherManager;
pub use server::Server;
pub use types::{MessageRecycle, RequestCrawl};
pub use validator::Manager as ValidatorManager;
