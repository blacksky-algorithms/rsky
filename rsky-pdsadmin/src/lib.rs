pub mod commands;
pub mod util;

use anyhow::Result;

/// The main entry point for the library functionality
/// This is primarily used for library consumers
pub fn run() -> Result<()> {
    commands::execute()
}
