#[expect(clippy::module_inception)]
mod server;
mod types;

pub use server::{Server, ServerError};
