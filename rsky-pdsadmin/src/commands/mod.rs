pub mod account;
pub mod create_invite_code;
pub mod request_crawl;
pub mod rsky_pds;
pub mod update;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use std::process::Command;
use which::which;

/// RSKY PDS Administration CLI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Enable verbose logging for additional debugging information
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Account management commands
    Account {
        #[command(subcommand)]
        subcommand: account::AccountCommands,
    },

    /// Create an invite code
    #[command(name = "create-invite-code")]
    CreateInviteCode,

    /// Request a crawl from a relay
    #[command(name = "request-crawl")]
    RequestCrawl {
        /// Comma-separated list of relay hosts
        #[arg(default_value = "")]
        relay_hosts: String,
    },

    /// Update the PDS to the latest version
    Update {
        /// Target version to update to (optional)
        #[arg(default_value = "")]
        target_version: String,
    },

    /// RSKY-PDS specific commands
    #[command(name = "rsky-pds")]
    RskyPds {
        #[command(subcommand)]
        subcommand: rsky_pds::RskyPdsCommands,
    },

    /// Display help information
    #[command(name = "show-help")]
    Help,

    /// External command that will be handled dynamically
    #[command(external_subcommand)]
    External(Vec<String>),
}

/// Set whether verbose logging is enabled
pub static mut VERBOSE_LOGGING: bool = false;

/// Check if verbose logging is enabled
pub fn is_verbose() -> bool {
    unsafe { VERBOSE_LOGGING }
}

/// Check if an external command exists in PATH
fn external_command_exists(command: &str) -> bool {
    which(format!("rsky-pdsadmin-{}", command)).is_ok()
}

/// Execute an external command
fn execute_external_command(command: &str, args: &[String]) -> Result<()> {
    let command_name = format!("rsky-pdsadmin-{}", command);

    // Find the command in PATH
    let command_path = which(&command_name)
        .with_context(|| format!("External command '{}' not found in PATH", command_name))?;

    if cfg!(target_family = "unix") {
        // On Unix-like systems, execute directly
        let status = Command::new(command_path)
            .args(args)
            .status()
            .with_context(|| format!("Failed to execute external command '{}'", command_name))?;

        if !status.success() {
            let exit_code = status.code().unwrap_or(-1);
            return Err(anyhow!(
                "External command '{}' failed with exit code {}",
                command_name,
                exit_code
            ));
        }
    } else {
        // On Windows, we might need additional handling
        let status = Command::new(command_path)
            .args(args)
            .status()
            .with_context(|| format!("Failed to execute external command '{}'", command_name))?;

        if !status.success() {
            let exit_code = status.code().unwrap_or(-1);
            return Err(anyhow!(
                "External command '{}' failed with exit code {}",
                command_name,
                exit_code
            ));
        }
    }

    Ok(())
}

/// Execute the CLI command
pub fn execute() -> Result<()> {
    let cli = Cli::parse();

    // Set the verbose flag for global use
    unsafe {
        VERBOSE_LOGGING = cli.verbose;
    }

    // If verbose mode is enabled, display it
    if cli.verbose {
        println!("Verbose mode enabled");
    }

    match &cli.command {
        Commands::Account { subcommand } => {
            account::execute(subcommand).context("Failed to execute account command")
        }
        Commands::CreateInviteCode => {
            create_invite_code::execute().context("Failed to create invite code")
        }
        Commands::RequestCrawl { relay_hosts } => {
            request_crawl::execute(relay_hosts).context("Failed to request crawl")
        }
        Commands::Update { target_version } => {
            update::execute(target_version).context("Failed to update PDS")
        }
        Commands::RskyPds { subcommand } => {
            rsky_pds::execute(subcommand).context("Failed to execute rsky-pds command")
        }
        Commands::Help => {
            print_help();
            Ok(())
        }
        Commands::External(args) => {
            if args.is_empty() {
                return Err(anyhow!("No external command specified"));
            }

            let command = &args[0];
            let command_args = &args[1..];

            if external_command_exists(command) {
                execute_external_command(command, command_args)
                    .with_context(|| format!("Failed to execute external command '{}'", command))
            } else {
                Err(anyhow!(
                    "Unknown command: {}. External command 'rsky-pdsadmin-{}' not found in PATH",
                    command,
                    command
                ))
            }
        }
    }
}

/// Print the help information
fn print_help() {
    println!("pdsadmin help");
    println!("--");
    println!("update");
    println!("  Update to the latest PDS version.");
    println!("    e.g. pdsadmin update");
    println!();
    println!("account");
    println!("  list");
    println!("    List accounts");
    println!("    e.g. pdsadmin account list");
    println!("  create <EMAIL> <HANDLE>");
    println!("    Create a new account");
    println!("    e.g. pdsadmin account create alice@example.com alice.example.com");
    println!("  delete <DID>");
    println!("    Delete an account specified by DID.");
    println!("    e.g. pdsadmin account delete did:plc:xyz123abc456");
    println!("  takedown <DID>");
    println!("    Takedown an account specified by DID.");
    println!("    e.g. pdsadmin account takedown did:plc:xyz123abc456");
    println!("  untakedown <DID>");
    println!("    Remove a takedown from an account specified by DID.");
    println!("    e.g. pdsadmin account untakedown did:plc:xyz123abc456");
    println!("  reset-password <DID>");
    println!("    Reset a password for an account specified by DID.");
    println!("    e.g. pdsadmin account reset-password did:plc:xyz123abc456");
    println!();
    println!("request-crawl [<RELAY HOST>]");
    println!("    Request a crawl from a relay host.");
    println!("    e.g. pdsadmin request-crawl bsky.network");
    println!();
    println!("create-invite-code");
    println!("  Create a new invite code.");
    println!("    e.g. pdsadmin create-invite-code");
    println!();
    println!("rsky-pds");
    println!("  init-db");
    println!("    Initialize the database with the required schema.");
    println!("    e.g. pdsadmin rsky-pds init-db");
    println!();
    println!("show-help");
    println!("    Display this help information.");
    println!();
    println!("External Commands");
    println!("    Any executable named 'rsky-pdsadmin-<command>' in your PATH");
    println!("    will be available as 'pdsadmin <command>'.");
    println!("    e.g. If 'rsky-pdsadmin-hello' exists in PATH, run it with 'pdsadmin hello'");
}
