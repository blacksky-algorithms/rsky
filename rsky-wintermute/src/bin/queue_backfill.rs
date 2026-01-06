use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use serde::Deserialize;

use rsky_wintermute::storage::Storage;
use rsky_wintermute::types::BackfillJob;

#[derive(Debug, Parser)]
#[command(name = "queue_backfill")]
#[command(about = "Queue DIDs for backfill from various sources")]
struct Args {
    /// Path to wintermute database directory
    #[arg(long, default_value = "backfill_cache")]
    db_path: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Queue DIDs from a CSV file (one DID per line)
    Csv {
        /// Path to CSV file containing DIDs
        #[arg(long)]
        file: PathBuf,
        /// Queue with high priority (processed before normal items)
        #[arg(long, default_value = "false")]
        priority: bool,
    },
    /// Queue all repos from a PDS server
    Pds {
        /// PDS host URL (e.g., blacksky.app or https://blacksky.app)
        #[arg(long)]
        host: String,
        /// Queue with high priority (processed before normal items)
        #[arg(long, default_value = "false")]
        priority: bool,
    },
    /// Queue specific DIDs (high priority by default)
    Dids {
        /// DIDs to queue (comma-separated or multiple --did flags)
        #[arg(long = "did", num_args = 1..)]
        dids: Vec<String>,
        /// Queue with normal priority instead of high priority
        #[arg(long, default_value = "false")]
        normal_priority: bool,
    },
    /// Show current queue status
    Status,
}

#[derive(Debug, Deserialize)]
struct ListReposResponse {
    repos: Vec<RepoRef>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RepoRef {
    did: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Opening database at {:?}", args.db_path);
    let storage = Arc::new(Storage::new(Some(args.db_path))?);

    match args.command {
        Command::Csv { file, priority } => queue_from_csv(&storage, &file, priority),
        Command::Pds { host, priority } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(queue_from_pds(&storage, &host, priority))
        }
        Command::Dids {
            dids,
            normal_priority,
        } => queue_dids(&storage, &dids, !normal_priority), // DIDs use priority by default
        Command::Status => show_status(&storage),
    }
}

fn queue_from_csv(storage: &Storage, path: &PathBuf, priority: bool) -> Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut queued = 0;
    let mut skipped = 0;

    let priority_str = if priority { "HIGH PRIORITY" } else { "normal" };
    println!("Queuing from CSV with {priority_str} priority");

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let did = line.trim();

        // Skip empty lines and header
        if did.is_empty() || did == "did" {
            skipped += 1;
            continue;
        }

        // Validate DID format
        if !did.starts_with("did:") {
            println!(
                "Warning: Skipping invalid DID at line {}: {}",
                line_num + 1,
                did
            );
            skipped += 1;
            continue;
        }

        let job = BackfillJob {
            did: did.to_string(),
            retry_count: 0,
        };

        if priority {
            storage.enqueue_backfill_priority(&job)?;
        } else {
            storage.enqueue_backfill(&job)?;
        }
        queued += 1;

        if queued % 1000 == 0 {
            println!("Queued {} DIDs...", queued);
        }
    }

    println!(
        "Done! Queued {} DIDs ({priority_str}), skipped {}",
        queued, skipped
    );
    println!("Current queue length: {}", storage.repo_backfill_len()?);

    Ok(())
}

async fn queue_from_pds(storage: &Storage, host: &str, priority: bool) -> Result<()> {
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    // Preserve scheme or default to https
    let (scheme, clean_hostname) = if host.starts_with("http://") {
        (
            "http",
            host.trim_start_matches("http://").trim_end_matches('/'),
        )
    } else {
        (
            "https",
            host.trim_start_matches("https://").trim_end_matches('/'),
        )
    };

    let mut cursor: Option<String> = None;
    let mut total_queued = 0;

    let priority_str = if priority { "HIGH PRIORITY" } else { "normal" };
    println!("Fetching repos from {scheme}://{clean_hostname} ({priority_str} priority)");

    loop {
        let mut url = url::Url::parse(&format!(
            "{scheme}://{clean_hostname}/xrpc/com.atproto.sync.listRepos"
        ))?;

        url.query_pairs_mut().append_pair("limit", "1000");
        if let Some(ref c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }

        println!("Fetching: {url}");

        let response = http_client.get(url.as_str()).send().await?;

        if !response.status().is_success() {
            return Err(color_eyre::eyre::eyre!("HTTP error: {}", response.status()));
        }

        let list_response: ListReposResponse = response.json().await?;

        println!("Received {} repos", list_response.repos.len());

        for repo in &list_response.repos {
            let job = BackfillJob {
                did: repo.did.clone(),
                retry_count: 0,
            };
            if priority {
                storage.enqueue_backfill_priority(&job)?;
            } else {
                storage.enqueue_backfill(&job)?;
            }
            total_queued += 1;

            if total_queued % 1000 == 0 {
                println!("Queued {} DIDs...", total_queued);
            }
        }

        if let Some(next_cursor) = list_response.cursor {
            cursor = Some(next_cursor);
        } else {
            break;
        }
    }

    println!(
        "Done! Queued {} DIDs from {} ({priority_str})",
        total_queued, clean_hostname
    );
    println!("Current queue length: {}", storage.repo_backfill_len()?);

    Ok(())
}

fn queue_dids(storage: &Storage, dids: &[String], priority: bool) -> Result<()> {
    let mut queued = 0;
    let mut skipped = 0;

    let priority_str = if priority { "HIGH PRIORITY" } else { "normal" };
    println!("Queuing DIDs with {priority_str} priority");

    for did_arg in dids {
        // Support comma-separated DIDs
        for did in did_arg.split(',') {
            let did = did.trim();

            if did.is_empty() {
                continue;
            }

            if !did.starts_with("did:") {
                println!("Warning: Skipping invalid DID: {}", did);
                skipped += 1;
                continue;
            }

            let job = BackfillJob {
                did: did.to_string(),
                retry_count: 0,
            };

            if priority {
                storage.enqueue_backfill_priority(&job)?;
            } else {
                storage.enqueue_backfill(&job)?;
            }
            queued += 1;
        }
    }

    println!(
        "Done! Queued {} DIDs ({priority_str}), skipped {}",
        queued, skipped
    );
    println!("Current queue length: {}", storage.repo_backfill_len()?);

    Ok(())
}

fn show_status(storage: &Storage) -> Result<()> {
    let repo_backfill_len = storage.repo_backfill_len()?;
    let firehose_live_len = storage.firehose_live_len()?;
    let firehose_backfill_len = storage.firehose_backfill_len()?;
    let label_live_len = storage.label_live_len()?;

    println!("Queue Status:");
    println!("  repo_backfill:     {}", repo_backfill_len);
    println!("  firehose_live:     {}", firehose_live_len);
    println!("  firehose_backfill: {}", firehose_backfill_len);
    println!("  label_live:        {}", label_live_len);

    Ok(())
}
