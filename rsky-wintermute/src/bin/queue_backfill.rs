use std::fs::File;
use std::io::{BufRead, BufReader, Write};
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
    /// Peek at the first N items in the repo_backfill queue
    Peek {
        /// Number of items to peek at
        #[arg(long, default_value = "10")]
        count: usize,
    },
    /// Search for a DID in the repo_backfill queue
    Search {
        /// DID to search for
        #[arg(long)]
        did: String,
        /// Maximum items to scan (default: entire queue)
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show detailed queue statistics
    Stats,
    /// Export repo_backfill queue to a file
    Export {
        /// Output file path
        #[arg(long)]
        file: PathBuf,
        /// Maximum items to export (default: all)
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Remove a specific DID from the repo_backfill queue
    Remove {
        /// DID to remove
        #[arg(long)]
        did: String,
    },
    /// Clear the entire repo_backfill queue (DANGEROUS)
    Clear {
        /// Confirm you want to clear the queue
        #[arg(long)]
        confirm: bool,
    },
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
        Command::Peek { count } => peek_queue(&storage, count),
        Command::Search { did, limit } => search_queue(&storage, &did, limit),
        Command::Stats => show_stats(&storage),
        Command::Export { file, limit } => export_queue(&storage, &file, limit),
        Command::Remove { did } => remove_from_queue(&storage, &did),
        Command::Clear { confirm } => clear_queue(&storage, confirm),
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
            priority,
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
                priority,
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
                priority,
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

fn peek_queue(storage: &Storage, count: usize) -> Result<()> {
    println!("Peeking at first {} items in repo_backfill queue:", count);
    println!();

    let items = storage.peek_backfill(count)?;

    if items.is_empty() {
        println!("Queue is empty");
        return Ok(());
    }

    for (i, (key, job)) in items.iter().enumerate() {
        let key_str = String::from_utf8_lossy(key);
        let priority = if key_str.starts_with("0:") {
            "HIGH"
        } else if key_str.starts_with("1:") {
            "normal"
        } else {
            "unknown"
        };
        println!(
            "{:3}. [{}] {} (retries: {})",
            i + 1,
            priority,
            job.did,
            job.retry_count
        );
        println!("     key: {}", key_str);
    }

    Ok(())
}

fn search_queue(storage: &Storage, did: &str, limit: Option<usize>) -> Result<()> {
    println!("Searching for DID: {}", did);
    println!();

    let max_scan = limit.unwrap_or(usize::MAX);
    let items = storage.peek_backfill(max_scan)?;

    let mut found = false;
    let mut scanned = 0;

    for (i, (key, job)) in items.iter().enumerate() {
        scanned += 1;
        if job.did == did || job.did.contains(did) {
            let key_str = String::from_utf8_lossy(key);
            let priority = if key_str.starts_with("0:") {
                "HIGH"
            } else if key_str.starts_with("1:") {
                "normal"
            } else {
                "unknown"
            };
            println!(
                "Found at position {}: [{}] {} (retries: {})",
                i + 1,
                priority,
                job.did,
                job.retry_count
            );
            println!("  key: {}", key_str);
            found = true;
        }
    }

    if !found {
        println!("DID not found in first {} items", scanned);
    }

    Ok(())
}

fn show_stats(storage: &Storage) -> Result<()> {
    let items = storage.peek_backfill(usize::MAX)?;
    let total = items.len();

    let mut priority_count = 0;
    let mut normal_count = 0;
    let mut retry_distribution: std::collections::HashMap<u32, usize> =
        std::collections::HashMap::new();

    for (key, job) in &items {
        let key_str = String::from_utf8_lossy(key);
        if key_str.starts_with("0:") {
            priority_count += 1;
        } else {
            normal_count += 1;
        }
        *retry_distribution.entry(job.retry_count).or_insert(0) += 1;
    }

    println!("Queue Statistics (repo_backfill):");
    println!("  Total items:     {}", total);
    println!("  High priority:   {}", priority_count);
    println!("  Normal priority: {}", normal_count);
    println!();
    println!("Retry distribution:");

    let mut retries: Vec<_> = retry_distribution.into_iter().collect();
    retries.sort_by_key(|(k, _)| *k);
    for (retry_count, count) in retries {
        println!("  {} retries: {} items", retry_count, count);
    }

    Ok(())
}

fn export_queue(storage: &Storage, path: &PathBuf, limit: Option<usize>) -> Result<()> {
    let max_export = limit.unwrap_or(usize::MAX);
    let items = storage.peek_backfill(max_export)?;

    let mut file = File::create(path)?;
    writeln!(file, "did,priority,retry_count")?;

    for (key, job) in &items {
        let key_str = String::from_utf8_lossy(key);
        let priority = if key_str.starts_with("0:") {
            "high"
        } else {
            "normal"
        };
        writeln!(file, "{},{},{}", job.did, priority, job.retry_count)?;
    }

    println!("Exported {} items to {:?}", items.len(), path);
    Ok(())
}

fn remove_from_queue(storage: &Storage, did: &str) -> Result<()> {
    println!("Searching for DID to remove: {}", did);

    let removed = storage.remove_backfill_by_did(did)?;

    if removed > 0 {
        println!("Removed {} entries for DID: {}", removed, did);
    } else {
        println!("DID not found in queue: {}", did);
    }

    println!("Current queue length: {}", storage.repo_backfill_len()?);
    Ok(())
}

fn clear_queue(storage: &Storage, confirm: bool) -> Result<()> {
    if !confirm {
        println!("This will DELETE ALL items in the repo_backfill queue!");
        println!("Current queue length: {}", storage.repo_backfill_len()?);
        println!();
        println!("To proceed, run with --confirm flag:");
        println!("  queue_backfill clear --confirm");
        return Ok(());
    }

    let count = storage.repo_backfill_len()?;
    storage.clear_repo_backfill()?;
    println!("Cleared {} items from repo_backfill queue", count);

    Ok(())
}
