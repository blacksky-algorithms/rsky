use anyhow::{Context, Result};
use clap::Subcommand;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use std::process::Command;

use crate::util::env;

// Define the embedded migrations using a relative path
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("../rsky-pds/migrations");

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

/// Initialize the database with the required schema
fn init_db() -> Result<()> {
    println!("Initializing RSKY-PDS database...");

    // Determine if diesel CLI is installed
    if !env::check_diesel_cli() {
        return Err(anyhow::anyhow!(
            "diesel CLI is not installed. Please install it with: cargo install diesel_cli --no-default-features --features postgres"
        ));
    }

    // Get the database URL
    let database_url = env::get_database_url()?;

    println!("Using database URL: {}", database_url);

    // Check if we're in a container
    if env::is_in_container() {
        // In container, we can directly run the migrations
        run_embedded_migrations(&database_url)
    } else {
        // Otherwise, run diesel CLI
        run_diesel_cli(&database_url)
    }
}

/// Run embedded migrations using diesel_migrations
fn run_embedded_migrations(database_url: &str) -> Result<()> {
    use diesel::Connection;
    use diesel::pg::PgConnection;

    // Connect to the database
    let mut conn =
        PgConnection::establish(database_url).context("Failed to connect to the database")?;

    // Run migrations
    let migration_result = conn.run_pending_migrations(MIGRATIONS);
    if migration_result.is_err() {
        return Err(anyhow::anyhow!("Failed to run migrations"));
    }

    println!("Database migrations completed successfully!");

    Ok(())
}

/// Run migrations using diesel CLI
fn run_diesel_cli(database_url: &str) -> Result<()> {
    // Find the path to the migrations directory (relative path)
    let migrations_dir = "../rsky-pds/migrations";

    // Set up the diesel CLI command
    let output = Command::new("diesel")
        .args([
            "migration",
            "run",
            "--database-url",
            database_url,
            "--migration-dir",
            migrations_dir,
        ])
        .output()
        .context("Failed to execute diesel migration command")?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Diesel migrations failed: {}", error));
    }

    println!("Database migrations completed successfully!");
    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
