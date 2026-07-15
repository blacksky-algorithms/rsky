use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum RskyPdsCommands {
    /// Initialize the database with the required schema
    #[command(name = "init-db")]
    InitDb,
}

pub fn execute(command: &RskyPdsCommands) -> Result<()> {
    match command {
        RskyPdsCommands::InitDb => init_db(),
    }
}

/// rsky-pds stores its data in SQLite and migrates automatically at
/// startup, so there is no separate schema initialization step.
fn init_db() -> Result<()> {
    println!(
        "rsky-pds runs its sqlite migrations automatically at startup; no manual init is required."
    );
    println!("Set PDS_DATA_DIRECTORY to control where the databases are created.");
    Ok(())
}
