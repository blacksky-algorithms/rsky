//! Direct-index pipeline: fetch CARs, walk MST, insert directly to PostgreSQL.
//!
//! Bypasses the LMDB record queue entirely. Each worker processes a DID+PDS pair
//! as a single atomic unit: fetch CAR → walk MST → extract records → COPY to PG.
//!
//! Usage:
//!   direct_index --file /tmp/did_pds.txt                   # DID|PDS pairs, pipe-delimited
//!   direct_index --file /tmp/dids.txt --resolve-pds        # DIDs only, resolve PDS via plc.directory
//!   direct_index --did did:plc:xxx --pds https://host.example.com  # Single DID

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::Parser;
use color_eyre::eyre::{self, Result};
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use rsky_repo::block_map::BlockMap;
use rsky_repo::parse::get_and_parse_record;
use rsky_repo::readable_repo::ReadableRepo;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use tokio::sync::{RwLock, Semaphore};
use tokio_postgres::NoTls;

const GLOBAL_CONCURRENCY: usize = 200;
const PER_HOST_CONCURRENCY: usize = 15;
const PG_POOL_SIZE: usize = 25;

#[derive(Parser)]
#[command(name = "direct_index")]
#[command(about = "Direct CAR→PG indexer, bypassing LMDB queue")]
struct Args {
    /// File containing DID|PDS pairs (pipe-delimited) or just DIDs (one per line)
    #[arg(long)]
    file: Option<std::path::PathBuf>,

    /// Single DID to process
    #[arg(long)]
    did: Option<String>,

    /// PDS endpoint for single DID mode
    #[arg(long)]
    pds: Option<String>,

    /// Resolve PDS endpoints via plc.directory (when file contains DIDs only)
    #[arg(long, default_value = "false")]
    resolve_pds: bool,

    /// Database URL
    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgresql://appview:2TP0Do4c50gK4O3OH3UwO9k5XX9oaQH3maP2rxWzZd0@localhost/appview_db?options=-csearch_path%3Dbsky"
    )]
    database_url: String,

    /// Only process follow records for this subject DID (optional, for targeted follower backfill)
    #[arg(long)]
    subject: Option<String>,
}

struct Counters {
    processed: AtomicU64,
    car_ok: AtomicU64,
    car_404: AtomicU64,
    car_429: AtomicU64,
    car_timeout: AtomicU64,
    car_other_err: AtomicU64,
    records_extracted: AtomicU64,
    records_inserted: AtomicU64,
    records_conflict: AtomicU64,
    subtrees_skipped: AtomicU64,
}

impl Counters {
    fn new() -> Self {
        Self {
            processed: AtomicU64::new(0),
            car_ok: AtomicU64::new(0),
            car_404: AtomicU64::new(0),
            car_429: AtomicU64::new(0),
            car_timeout: AtomicU64::new(0),
            car_other_err: AtomicU64::new(0),
            records_extracted: AtomicU64::new(0),
            records_inserted: AtomicU64::new(0),
            records_conflict: AtomicU64::new(0),
            subtrees_skipped: AtomicU64::new(0),
        }
    }
}

async fn fetch_car(http: &reqwest::Client, url: &str) -> Result<bytes::Bytes, (u16, String)> {
    for attempt in 0..3u32 {
        match http.get(url).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if resp.status().is_success() {
                    return resp.bytes().await.map_err(|e| (0, format!("body: {e}")));
                }
                if status == 429 || status == 503 {
                    tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
                    continue;
                }
                return Err((status, format!("HTTP {status}")));
            }
            Err(e) => {
                if e.is_timeout() {
                    return Err((0, "timeout".into()));
                }
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
                return Err((0, format!("{e}")));
            }
        }
    }
    Err((0, "max retries".into()))
}

async fn process_did(
    did: &str,
    pds: &str,
    pool: &Pool,
    http: &reqwest::Client,
    subject_filter: Option<&str>,
    counters: &Counters,
) -> Result<()> {
    let car_url = format!("{}/xrpc/com.atproto.sync.getRepo?did={}", pds, did);
    let car_bytes = match fetch_car(http, &car_url).await {
        Ok(b) => b,
        Err((status, msg)) => {
            match status {
                404 => counters.car_404.fetch_add(1, Ordering::Relaxed),
                429 => counters.car_429.fetch_add(1, Ordering::Relaxed),
                0 if msg == "timeout" => counters.car_timeout.fetch_add(1, Ordering::Relaxed),
                _ => counters.car_other_err.fetch_add(1, Ordering::Relaxed),
            };
            return Ok(());
        }
    };

    // Parse CAR
    let mut reader = iroh_car::CarReader::new(Cursor::new(car_bytes.to_vec())).await?;
    let root = *reader
        .header()
        .roots()
        .first()
        .ok_or_else(|| eyre::eyre!("no root"))?;
    let mut blocks = BlockMap::new();
    while let Some((cid, data)) = reader.next_block().await? {
        blocks.set(cid, data.clone());
    }

    // Walk MST
    let blockstore = MemoryBlockstore::new(Some(blocks))
        .await
        .map_err(|e| eyre::eyre!("blockstore: {e}"))?;
    let storage_arc = Arc::new(tokio::sync::RwLock::new(blockstore));
    let repo = ReadableRepo::load(storage_arc.clone(), root)
        .await
        .map_err(|e| eyre::eyre!("repo load: {e}"))?;
    let repo_storage = repo.storage.clone();
    let leaves = repo
        .data
        .reachable_leaves()
        .await
        .map_err(|e| eyre::eyre!("reachable_leaves: {e}"))?;

    let blocks_result = {
        let g = repo_storage.read().await;
        g.get_blocks(leaves.iter().map(|e| e.value).collect())
            .await
            .map_err(|e| eyre::eyre!("get_blocks: {e}"))?
    };

    // Get PG connection
    let client = pool.get().await?;
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();

    for entry in &leaves {
        let Some((collection, rkey)) = entry.key.split_once('/') else {
            continue;
        };
        if !collection.starts_with("app.bsky.") && !collection.starts_with("chat.bsky.") {
            continue;
        }

        let Ok(parsed) = get_and_parse_record(&blocks_result.blocks, entry.value) else {
            continue;
        };

        let record_json = serde_json::to_value(&parsed.record)?;
        counters.records_extracted.fetch_add(1, Ordering::Relaxed);

        let uri = format!("at://{did}/{collection}/{rkey}");
        let cid = entry.value.to_string();

        // Handle follow records
        if collection == "app.bsky.graph.follow" {
            let subject = record_json
                .get("subject")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // If subject filter is set, only process follows to that subject
            if let Some(filter) = subject_filter {
                if subject != filter {
                    continue;
                }
            }

            let created_at = record_json
                .get("createdAt")
                .and_then(|v| v.as_str())
                .unwrap_or(&now)
                .to_string();

            let rows = client
                .execute(
                    "INSERT INTO follow (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\") \
                     VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
                    &[
                        &uri,
                        &cid,
                        &did.to_string(),
                        &subject.to_string(),
                        &created_at,
                        &now,
                    ],
                )
                .await?;

            if rows > 0 {
                counters.records_inserted.fetch_add(1, Ordering::Relaxed);
            } else {
                counters.records_conflict.fetch_add(1, Ordering::Relaxed);
            }
        }
        // TODO: Add handlers for other collection types (post, like, repost, etc.)
        // For now, this binary focuses on follow records for targeted follower backfill.
    }

    counters.car_ok.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Build DID+PDS list
    let did_pds: Vec<(String, String)> = if let Some(file) = &args.file {
        let content = std::fs::read_to_string(file)?;
        content
            .lines()
            .filter_map(|l| {
                if let Some((d, p)) = l.split_once('|') {
                    Some((d.to_string(), p.to_string()))
                } else if l.starts_with("did:") && args.resolve_pds {
                    // TODO: resolve PDS via plc.directory
                    tracing::warn!("PDS resolution not yet implemented, skipping {}", l);
                    None
                } else {
                    None
                }
            })
            .collect()
    } else if let (Some(did), Some(pds)) = (&args.did, &args.pds) {
        vec![(did.clone(), pds.clone())]
    } else {
        eyre::bail!("Provide --file or --did + --pds");
    };

    let total = did_pds.len() as u64;
    println!("DIDs to process: {}", total);
    if let Some(ref s) = args.subject {
        println!("Subject filter: {}", s);
    }

    // PG pool
    let mut pg_config = Config::new();
    pg_config.url = Some(args.database_url.clone());
    pg_config.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    pg_config.pool = Some(deadpool_postgres::PoolConfig::new(PG_POOL_SIZE));
    let pool = pg_config.create_pool(Some(Runtime::Tokio1), NoTls)?;

    // Check subject follower count before
    let subject_before = if let Some(ref s) = args.subject {
        let client = pool.get().await?;
        let row = client
            .query_one(
                "SELECT \"followersCount\" FROM profile_agg WHERE did = $1",
                &[s],
            )
            .await?;
        let count: i32 = row.get(0);
        println!("Subject followers BEFORE: {}", count);
        Some(count)
    } else {
        None
    };

    // Concurrency control
    let host_semas: Arc<RwLock<HashMap<String, Arc<Semaphore>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let global_sema = Arc::new(Semaphore::new(GLOBAL_CONCURRENCY));
    let counters = Arc::new(Counters::new());
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let mut handles = Vec::new();

    for (did, pds) in did_pds {
        let global_permit = global_sema.clone().acquire_owned().await?;
        let host = pds.clone();
        let host_sema = {
            let mut map = host_semas.write().await;
            map.entry(host)
                .or_insert_with(|| Arc::new(Semaphore::new(PER_HOST_CONCURRENCY)))
                .clone()
        };
        let host_permit = host_sema.acquire_owned().await?;

        let pool = pool.clone();
        let http = http.clone();
        let counters = counters.clone();
        let subject = args.subject.clone();

        handles.push(tokio::spawn(async move {
            let _gp = global_permit;
            let _hp = host_permit;

            if let Err(e) = process_did(
                &did,
                &pds,
                &pool,
                &http,
                subject.as_deref(),
                &counters,
            )
            .await
            {
                counters.car_other_err.fetch_add(1, Ordering::Relaxed);
                let p = counters.processed.load(Ordering::Relaxed);
                if p < 10 {
                    eprintln!("Error {}: {}", did, e);
                }
            }

            let p = counters.processed.fetch_add(1, Ordering::Relaxed) + 1;
            if p % 200 == 0 || p == total {
                eprintln!(
                    "[{}/{}] ok={} 404={} 429={} timeout={} err={} extracted={} inserted={} conflict={} subtrees_skipped={}",
                    p, total,
                    counters.car_ok.load(Ordering::Relaxed),
                    counters.car_404.load(Ordering::Relaxed),
                    counters.car_429.load(Ordering::Relaxed),
                    counters.car_timeout.load(Ordering::Relaxed),
                    counters.car_other_err.load(Ordering::Relaxed),
                    counters.records_extracted.load(Ordering::Relaxed),
                    counters.records_inserted.load(Ordering::Relaxed),
                    counters.records_conflict.load(Ordering::Relaxed),
                    counters.subtrees_skipped.load(Ordering::Relaxed),
                );
            }
        }));
    }

    for h in handles {
        h.await.ok();
    }

    // Corrective profile_agg COUNT for subject
    if let Some(ref s) = args.subject {
        let client = pool.get().await?;
        client
            .execute(
                "INSERT INTO profile_agg (did, \"followersCount\") \
                 SELECT $1::varchar, COUNT(*) FROM follow WHERE \"subjectDid\" = $1 \
                 ON CONFLICT (did) DO UPDATE SET \"followersCount\" = EXCLUDED.\"followersCount\"",
                &[s],
            )
            .await?;
        let row = client
            .query_one(
                "SELECT \"followersCount\" FROM profile_agg WHERE did = $1",
                &[s],
            )
            .await?;
        let after: i32 = row.get(0);
        println!("Subject followers AFTER: {}", after);
        if let Some(before) = subject_before {
            println!("Delta: +{}", after - before);
        }
    }

    println!("\n=== RESULTS ===");
    println!(
        "DIDs processed:       {}",
        counters.processed.load(Ordering::Relaxed)
    );
    println!(
        "CARs fetched OK:      {}",
        counters.car_ok.load(Ordering::Relaxed)
    );
    println!(
        "CARs 404:             {}",
        counters.car_404.load(Ordering::Relaxed)
    );
    println!(
        "CARs 429:             {}",
        counters.car_429.load(Ordering::Relaxed)
    );
    println!(
        "CARs timeout:         {}",
        counters.car_timeout.load(Ordering::Relaxed)
    );
    println!(
        "CARs other error:     {}",
        counters.car_other_err.load(Ordering::Relaxed)
    );
    println!(
        "Records extracted:    {}",
        counters.records_extracted.load(Ordering::Relaxed)
    );
    println!(
        "Records inserted:     {}",
        counters.records_inserted.load(Ordering::Relaxed)
    );
    println!(
        "Records conflict:     {}",
        counters.records_conflict.load(Ordering::Relaxed)
    );
    println!(
        "MST subtrees skipped: {}",
        counters.subtrees_skipped.load(Ordering::Relaxed)
    );

    Ok(())
}
