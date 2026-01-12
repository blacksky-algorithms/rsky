//! Migration tool to fix corrupted blob references in the record table.
//!
//! Converts byte arrays `[1, 85, 18, ...]` to proper IPLD format `{"$link": "bafyrei..."}`.

use clap::Parser;
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use lexicon_cid::Cid;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio_postgres::NoTls;

#[derive(Parser)]
#[command(name = "fix_blob_refs")]
#[command(about = "Fix corrupted blob references in record table")]
struct Args {
    /// PostgreSQL connection URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// Number of records to process per batch
    #[arg(long, default_value = "1000")]
    batch_size: usize,

    /// Number of concurrent workers
    #[arg(long, default_value = "10")]
    workers: usize,

    /// Dry run - don't actually update records
    #[arg(long)]
    dry_run: bool,

    /// Resume from this URI (for continuation after interruption)
    #[arg(long)]
    resume_from: Option<String>,

    /// Filter by record type (e.g., "app.bsky.feed.post", "app.bsky.actor.profile")
    #[arg(long)]
    filter: Option<String>,

    /// Maximum number of records to process (0 = unlimited)
    #[arg(long, default_value = "0")]
    limit: u64,
}

/// Convert byte arrays in JSON to CID links (reuses backfiller logic)
fn convert_record_to_ipld(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), convert_record_to_ipld(v));
            }
            Value::Object(new_map)
        }
        Value::Array(arr) => {
            // Check if this is a byte array (potential CID)
            let is_byte_array = arr
                .iter()
                .all(|v| matches!(v, Value::Number(n) if n.as_u64().is_some_and(|num| num <= 255)));

            if is_byte_array && !arr.is_empty() {
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                    .collect();

                if let Ok(cid) = Cid::try_from(&bytes[..]) {
                    return serde_json::json!({"$link": cid.to_string()});
                }
            }

            Value::Array(arr.iter().map(convert_record_to_ipld).collect())
        }
        other => other.clone(),
    }
}

/// Check if a JSON value contains any byte arrays that look like CIDs
fn has_corrupted_blob_refs(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.values().any(has_corrupted_blob_refs),
        Value::Array(arr) => {
            // Check if this array itself is a byte array
            let is_byte_array = arr
                .iter()
                .all(|v| matches!(v, Value::Number(n) if n.as_u64().is_some_and(|num| num <= 255)));

            if is_byte_array && !arr.is_empty() {
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                    .collect();

                // Check if it's a valid CID
                if Cid::try_from(&bytes[..]).is_ok() {
                    return true;
                }
            }

            // Recursively check array elements
            arr.iter().any(has_corrupted_blob_refs)
        }
        _ => false,
    }
}

struct Stats {
    processed: AtomicU64,
    fixed: AtomicU64,
    skipped: AtomicU64,
    errors: AtomicU64,
    start_time: Instant,
}

impl Stats {
    fn new() -> Self {
        Self {
            processed: AtomicU64::new(0),
            fixed: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    fn report(&self) {
        let processed = self.processed.load(Ordering::Relaxed);
        let fixed = self.fixed.load(Ordering::Relaxed);
        let skipped = self.skipped.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let rate = if elapsed > 0.0 {
            processed as f64 / elapsed
        } else {
            0.0
        };

        println!(
            "Progress: {} processed, {} fixed, {} skipped, {} errors ({:.1}/s)",
            processed, fixed, skipped, errors, rate
        );
    }
}

async fn create_pool(
    database_url: &str,
    pool_size: usize,
) -> Result<Pool, Box<dyn std::error::Error>> {
    let mut cfg = Config::new();
    cfg.url = Some(database_url.to_string());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });

    let pool = cfg
        .builder(NoTls)?
        .max_size(pool_size)
        .runtime(Runtime::Tokio1)
        .build()?;

    Ok(pool)
}

async fn get_affected_uris(
    pool: &Pool,
    last_uri: Option<&str>,
    batch_size: usize,
    filter: Option<&str>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let client = pool.get().await?;

    let query = match (last_uri, filter) {
        (Some(uri), Some(f)) => {
            client
                .query(
                    "SELECT uri FROM bsky.record
                     WHERE json::text LIKE '%\"ref\":[1,%'
                     AND uri > $1
                     AND uri LIKE $2
                     ORDER BY uri
                     LIMIT $3",
                    &[&uri, &format!("%{}%", f), &(batch_size as i64)],
                )
                .await?
        }
        (Some(uri), None) => {
            client
                .query(
                    "SELECT uri FROM bsky.record
                     WHERE json::text LIKE '%\"ref\":[1,%'
                     AND uri > $1
                     ORDER BY uri
                     LIMIT $2",
                    &[&uri, &(batch_size as i64)],
                )
                .await?
        }
        (None, Some(f)) => {
            client
                .query(
                    "SELECT uri FROM bsky.record
                     WHERE json::text LIKE '%\"ref\":[1,%'
                     AND uri LIKE $1
                     ORDER BY uri
                     LIMIT $2",
                    &[&format!("%{}%", f), &(batch_size as i64)],
                )
                .await?
        }
        (None, None) => {
            client
                .query(
                    "SELECT uri FROM bsky.record
                     WHERE json::text LIKE '%\"ref\":[1,%'
                     ORDER BY uri
                     LIMIT $1",
                    &[&(batch_size as i64)],
                )
                .await?
        }
    };

    Ok(query.iter().map(|row| row.get::<_, String>(0)).collect())
}

async fn process_uri(
    pool: &Pool,
    uri: &str,
    dry_run: bool,
    stats: &Stats,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = pool.get().await?;

    let row = client
        .query_opt("SELECT json FROM bsky.record WHERE uri = $1", &[&uri])
        .await?;

    let Some(row) = row else {
        stats.skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    };

    let json_text: String = row.get(0);
    let json: Value = serde_json::from_str(&json_text)
        .map_err(|e| format!("Failed to parse JSON for {uri}: {e}"))?;

    // Check if it actually needs fixing
    if !has_corrupted_blob_refs(&json) {
        stats.skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    let fixed_json = convert_record_to_ipld(&json);

    if json == fixed_json {
        stats.skipped.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    if !dry_run {
        let fixed_json_text = serde_json::to_string(&fixed_json)?;
        client
            .execute(
                "UPDATE bsky.record SET json = $1 WHERE uri = $2",
                &[&fixed_json_text, &uri],
            )
            .await?;
    }

    stats.fixed.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("Fix Blob Refs Migration Tool");
    println!("============================");
    println!(
        "Database: {}",
        args.database_url.split('@').last().unwrap_or("*****")
    );
    println!("Batch size: {}", args.batch_size);
    println!("Workers: {}", args.workers);
    println!("Dry run: {}", args.dry_run);
    if let Some(ref filter) = args.filter {
        println!("Filter: {}", filter);
    }
    if let Some(ref resume) = args.resume_from {
        println!("Resume from: {}", resume);
    }
    if args.limit > 0 {
        println!("Limit: {}", args.limit);
    }
    println!();

    let pool = create_pool(&args.database_url, args.workers + 2).await?;
    let stats = Arc::new(Stats::new());

    // Test connection
    {
        let client = pool.get().await?;
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM bsky.record WHERE json::text LIKE '%\"ref\":[1,%'",
                &[],
            )
            .await?;
        let total: i64 = row.get(0);
        println!("Total affected records: {}", total);
        println!();
    }

    let mut last_uri = args.resume_from.clone();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(args.workers));

    loop {
        let uris = get_affected_uris(
            &pool,
            last_uri.as_deref(),
            args.batch_size,
            args.filter.as_deref(),
        )
        .await?;

        if uris.is_empty() {
            break;
        }

        last_uri = uris.last().cloned();

        let mut handles = Vec::new();

        for uri in uris {
            let pool = pool.clone();
            let stats = Arc::clone(&stats);
            let permit = semaphore.clone().acquire_owned().await?;
            let dry_run = args.dry_run;

            let handle = tokio::spawn(async move {
                let result = process_uri(&pool, &uri, dry_run, &stats).await;
                stats.processed.fetch_add(1, Ordering::Relaxed);
                drop(permit);

                if let Err(e) = result {
                    stats.errors.fetch_add(1, Ordering::Relaxed);
                    eprintln!("Error processing {}: {}", uri, e);
                }
            });

            handles.push(handle);
        }

        // Wait for batch to complete
        for handle in handles {
            let _ = handle.await;
        }

        stats.report();

        // Check limit
        if args.limit > 0 && stats.processed.load(Ordering::Relaxed) >= args.limit {
            println!("Reached limit of {} records", args.limit);
            break;
        }

        // Save checkpoint
        if let Some(ref uri) = last_uri {
            println!("Checkpoint: {}", uri);
        }
    }

    println!();
    println!("Migration complete!");
    stats.report();

    if args.dry_run {
        println!();
        println!("This was a dry run. No records were modified.");
        println!("Run without --dry-run to apply changes.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_record_to_ipld_with_byte_array() {
        // CID bytes for bafkreie34ehrn7eczwyvzqqcu3k3b5vx3axjhs4l7q5angiu57r7za7x6i
        let input = json!({
            "image": {
                "$type": "blob",
                "ref": [1, 85, 18, 32, 155, 225, 15, 22, 252, 130, 205, 177, 92, 194, 2, 166, 213, 176, 246, 183, 216, 46, 147, 203, 139, 252, 58, 6, 153, 20, 239, 227, 252, 131, 247, 242],
                "mimeType": "image/jpeg",
                "size": 350016
            }
        });

        let output = convert_record_to_ipld(&input);

        // Should have converted the ref to a $link
        assert!(output["image"]["ref"]["$link"].is_string());
        let link = output["image"]["ref"]["$link"].as_str().unwrap();
        assert!(link.starts_with("baf"));
    }

    #[test]
    fn test_convert_record_to_ipld_already_correct() {
        let input = json!({
            "image": {
                "$type": "blob",
                "ref": {"$link": "bafkreie34ehrn7eczwyvzqqcu3k3b5vx3axjhs4l7q5angiu57r7za7x6i"},
                "mimeType": "image/jpeg",
                "size": 350016
            }
        });

        let output = convert_record_to_ipld(&input);

        // Should be unchanged
        assert_eq!(input, output);
    }

    #[test]
    fn test_has_corrupted_blob_refs_true() {
        let input = json!({
            "embed": {
                "images": [{
                    "image": {
                        "ref": [1, 85, 18, 32, 155, 225, 15, 22, 252, 130, 205, 177, 92, 194, 2, 166, 213, 176, 246, 183, 216, 46, 147, 203, 139, 252, 58, 6, 153, 20, 239, 227, 252, 131, 247, 242]
                    }
                }]
            }
        });

        assert!(has_corrupted_blob_refs(&input));
    }

    #[test]
    fn test_has_corrupted_blob_refs_false() {
        let input = json!({
            "embed": {
                "images": [{
                    "image": {
                        "ref": {"$link": "bafkreie34ehrn7eczwyvzqqcu3k3b5vx3axjhs4l7q5angiu57r7za7x6i"}
                    }
                }]
            }
        });

        assert!(!has_corrupted_blob_refs(&input));
    }

    #[test]
    fn test_regular_array_not_converted() {
        let input = json!({
            "langs": ["en", "es"],
            "tags": ["test", "example"]
        });

        let output = convert_record_to_ipld(&input);

        // String arrays should be unchanged
        assert_eq!(input, output);
    }
}
