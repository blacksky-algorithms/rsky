/// Module version of lib.rs
mod error;
mod header;
mod reader;
mod util;
mod writer;

pub use header::CarHeader;
//pub use reader::CarReader;
pub use writer::CarWriter;
