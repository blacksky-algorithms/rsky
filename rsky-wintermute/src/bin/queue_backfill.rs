use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use color_eyre::Result;

use rsky_wintermute::storage::Storage;
use rsky_wintermute::types::BackfillJob;

#[derive(Debug, Parser)]
#[command(name = "queue_backfill")]
#[command(about = "Queue DIDs for backfill from a CSV file")]
struct Args {
    /// Path to CSV file containing DIDs (one per line, with optional header)
    #[arg(long)]
    csv: PathBuf,

    /// Path to wintermute database directory
    #[arg(long, default_value = "wintermute_db")]
    db_path: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Opening database at {:?}", args.db_path);
    let storage = Arc::new(Storage::new(Some(args.db_path))?);

    let file = File::open(&args.csv)?;
    let reader = BufReader::new(file);

    let mut queued = 0;
    let mut skipped = 0;

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

        storage.enqueue_backfill(&job)?;
        queued += 1;

        if queued % 1000 == 0 {
            println!("Queued {} DIDs...", queued);
        }
    }

    println!("Done! Queued {} DIDs, skipped {}", queued, skipped);
    println!("Current queue length: {}", storage.repo_backfill_len()?);

    Ok(())
}
