//! Import handle claims from the PLC directory export into the actor table.
//!
//! Streams `https://plc.directory/export` with pagination, extracts
//! `did` + `alsoKnownAs` handle from each operation, and bulk-updates
//! actors that currently have NULL handles using the COPY protocol.
//!
//! Imported handles are marked with `indexedAt = 1970-01-01T00:00:01Z`
//! so the normal handle resolution loop still verifies them bidirectionally.

use clap::Parser;
use color_eyre::eyre::{Context, Result};
use futures::{SinkExt, pin_mut};
use std::io::Write;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

const PLC_EXPORT_URL: &str = "https://plc.directory/export";
const PAGE_SIZE: usize = 1000;
/// Delay between pages to avoid overwhelming PLC directory
const PAGE_DELAY: Duration = Duration::from_millis(200);
/// Batch size for COPY operations
const COPY_BATCH_SIZE: usize = 5000;
/// indexedAt value for PLC-imported handles (unverified)
const UNVERIFIED_INDEXED_AT: &str = "1970-01-01T00:00:01Z";

#[derive(Parser)]
#[command(
    name = "plc_import",
    about = "Import handle claims from PLC directory into actor table"
)]
struct Args {
    /// PostgreSQL connection URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// Resume from this cursor (createdAt timestamp from last successful page)
    #[arg(long)]
    after: Option<String>,

    /// Only update actors with NULL handles (default: true)
    #[arg(long, default_value = "true")]
    null_only: bool,

    /// Dry run: count operations without writing to DB
    #[arg(long, default_value = "false")]
    dry_run: bool,
}

/// A PLC operation from the export endpoint
#[derive(serde::Deserialize)]
struct PlcOperation {
    did: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[allow(dead_code)]
    nullified: bool,
    operation: PlcOperationBody,
}

#[derive(serde::Deserialize)]
struct PlcOperationBody {
    #[serde(rename = "alsoKnownAs", default)]
    also_known_as: Vec<String>,
}

/// Extract handle from alsoKnownAs array (format: "at://handle.example.com")
fn extract_handle(also_known_as: &[String]) -> Option<String> {
    also_known_as
        .iter()
        .find(|s| s.starts_with("at://"))
        .map(|s| s.trim_start_matches("at://").to_lowercase())
        .filter(|h| !h.is_empty())
}

async fn connect_pg(database_url: &str) -> Result<tokio_postgres::Client> {
    let (client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
        .await
        .wrap_err("failed to connect to PostgreSQL")?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            error!(error = %e, "PostgreSQL connection error");
        }
    });

    // Disable statement timeout for bulk operations
    client
        .execute("SET statement_timeout = 0", &[])
        .await
        .wrap_err("failed to disable statement timeout")?;

    Ok(client)
}

/// Deduplicate a batch of (did, handle) pairs.
/// For duplicate DIDs, keep the last entry (most recent PLC operation).
/// For duplicate handles, keep the last entry (most recent claimer).
fn dedup_batch(data: &[(String, String)]) -> Vec<(String, String)> {
    use std::collections::HashMap;
    // Last-write-wins for both did and handle
    let mut by_did: HashMap<&str, &str> = HashMap::new();
    for (did, handle) in data {
        by_did.insert(did.as_str(), handle.as_str());
    }
    // Also deduplicate by handle: if two DIDs claim the same handle, keep the last one
    let mut by_handle: HashMap<&str, &str> = HashMap::new();
    for (did, handle) in &by_did {
        by_handle.insert(*handle, *did);
    }
    by_handle
        .into_iter()
        .map(|(handle, did)| (did.to_owned(), handle.to_owned()))
        .collect()
}

/// Bulk-update actor handles using COPY protocol.
/// Only updates actors with NULL handles if null_only is true.
/// Returns the number of actors updated.
async fn copy_update_handles(
    client: &tokio_postgres::Client,
    data: &[(String, String)], // (did, handle)
    null_only: bool,
) -> Result<u64> {
    if data.is_empty() {
        return Ok(0);
    }

    let data = dedup_batch(data);

    // Create temp table
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _plc_handles (
                did text NOT NULL,
                handle text NOT NULL
            )",
            &[],
        )
        .await
        .wrap_err("failed to create temp table")?;

    client
        .execute("TRUNCATE _plc_handles", &[])
        .await
        .wrap_err("failed to truncate temp table")?;

    // COPY data into temp table
    let copy_stmt = client
        .copy_in("COPY _plc_handles (did, handle) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await
        .wrap_err("failed to start COPY")?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 100);
    for (did, handle) in data {
        // Escape backslash and tab for COPY text format
        let escaped_handle = handle.replace('\\', "\\\\").replace('\t', "\\t");
        writeln!(buffer, "{did}\t{escaped_handle}").wrap_err("buffer write error")?;
    }

    sink.send(bytes::Bytes::from(buffer))
        .await
        .wrap_err("failed to send COPY data")?;
    sink.close().await.wrap_err("failed to close COPY sink")?;

    // UPDATE actors with handles from temp table.
    // Exclude handles already assigned to other actors (unique constraint on handle).
    let query = if null_only {
        "UPDATE actor SET handle = p.handle, \"indexedAt\" = $1
         FROM _plc_handles p
         WHERE actor.did = p.did AND actor.handle IS NULL
           AND NOT EXISTS (
             SELECT 1 FROM actor a2 WHERE a2.handle = p.handle AND a2.did != p.did
           )"
    } else {
        "UPDATE actor SET handle = p.handle, \"indexedAt\" = $1
         FROM _plc_handles p
         WHERE actor.did = p.did
           AND NOT EXISTS (
             SELECT 1 FROM actor a2 WHERE a2.handle = p.handle AND a2.did != p.did
           )"
    };

    let updated = client
        .execute(query, &[&UNVERIFIED_INDEXED_AT])
        .await
        .wrap_err("failed to update actor handles")?;

    Ok(updated)
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Install default rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let args = Args::parse();

    info!("starting PLC directory import");
    info!(
        null_only = args.null_only,
        dry_run = args.dry_run,
        "configuration"
    );

    let pg = if !args.dry_run {
        Some(connect_pg(&args.database_url).await?)
    } else {
        None
    };

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("rsky-wintermute/plc-import")
        .build()
        .wrap_err("failed to build HTTP client")?;

    let mut cursor = args.after.clone();
    let mut total_operations = 0u64;
    let mut total_handles_found = 0u64;
    let mut total_updated = 0u64;
    let mut page_count = 0u64;
    let mut batch: Vec<(String, String)> = Vec::with_capacity(COPY_BATCH_SIZE);
    let start = Instant::now();

    loop {
        // Build URL with pagination
        let mut url = format!("{PLC_EXPORT_URL}?count={PAGE_SIZE}");
        if let Some(ref after) = cursor {
            url.push_str(&format!("&after={after}"));
        }

        // Fetch page
        let response = match http.get(&url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!(error = %e, "failed to fetch PLC export page, retrying in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            warn!(%status, "PLC export returned error, retrying in 5s");
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        let body = response
            .text()
            .await
            .wrap_err("failed to read PLC export response body")?;

        // Parse newline-delimited JSON
        let operations: Vec<PlcOperation> = body
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| match serde_json::from_str::<PlcOperation>(line) {
                Ok(op) => Some(op),
                Err(e) => {
                    tracing::debug!(error = %e, "skipping unparseable PLC operation");
                    None
                }
            })
            .collect();

        if operations.is_empty() {
            info!("no more operations from PLC directory");
            break;
        }

        page_count += 1;
        let page_ops = operations.len();
        total_operations += page_ops as u64;

        // Extract (did, handle) pairs from operations
        for op in &operations {
            if op.nullified {
                continue;
            }
            if let Some(handle) = extract_handle(&op.operation.also_known_as) {
                // Only process did:plc DIDs from the PLC directory
                if op.did.starts_with("did:plc:") {
                    total_handles_found += 1;
                    batch.push((op.did.clone(), handle));
                }
            }
        }

        // Update cursor to last operation's timestamp
        if let Some(last) = operations.last() {
            cursor = Some(last.created_at.clone());
        }

        // Flush batch when large enough
        if batch.len() >= COPY_BATCH_SIZE {
            if let Some(ref pg) = pg {
                let updated = copy_update_handles(pg, &batch, args.null_only).await?;
                total_updated += updated;
            }
            batch.clear();
        }

        // Log progress every 100 pages
        if page_count % 100 == 0 {
            let elapsed = start.elapsed().as_secs();
            let rate = if elapsed > 0 {
                total_operations / elapsed
            } else {
                0
            };
            info!(
                page_count,
                total_operations,
                total_handles_found,
                total_updated,
                ops_per_sec = rate,
                cursor = cursor.as_deref().unwrap_or("none"),
                "PLC import progress"
            );
        }

        // End of data check
        if page_ops < PAGE_SIZE {
            info!("reached end of PLC directory export");
            break;
        }

        tokio::time::sleep(PAGE_DELAY).await;
    }

    // Flush remaining batch
    if !batch.is_empty() {
        if let Some(ref pg) = pg {
            let updated = copy_update_handles(pg, &batch, args.null_only).await?;
            total_updated += updated;
        }
    }

    let elapsed = start.elapsed().as_secs();
    info!(
        page_count,
        total_operations,
        total_handles_found,
        total_updated,
        elapsed_secs = elapsed,
        "PLC directory import complete"
    );

    Ok(())
}
