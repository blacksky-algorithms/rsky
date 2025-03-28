/// Module version of lib.rs
pub mod error;
mod header;
pub mod reader;
mod util;
mod writer;

pub use header::CarHeader;
pub use reader::CarReader;
pub use writer::CarWriter;
