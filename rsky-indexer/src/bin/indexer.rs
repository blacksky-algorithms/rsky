use anyhow::Result;
use clap::Parser;
use deadpool_postgres::{
    Config, ManagerConfig, Pool, PoolConfig, RecyclingMethod, Runtime, Timeouts,
};
use lexicon_cid::Cid;
use rsky_indexer::{
    indexing::IndexingService, label_indexer::LabelIndexer, metrics, stream_indexer::StreamIndexer,
    streams, IndexerConfig,
};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio_postgres::NoTls;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use warp::Filter;

/// Convert record JSON to IPLD format
fn convert_record_to_ipld(record_json: &serde_json::Value) -> serde_json::Value {
    match record_json {
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map.iter() {
                new_map.insert(k.clone(), convert_record_to_ipld(v));
            }
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            let is_byte_array = arr.iter().all(|v| {
                matches!(v, serde_json::Value::Number(n) if n.as_u64().map_or(false, |num| num <= 255))
            });

            if is_byte_array && !arr.is_empty() {
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();

                if let Ok(cid) = Cid::try_from(&bytes[..]) {
                    return serde_json::json!({"$link": cid.to_string()});
                }
            }

            serde_json::Value::Array(arr.iter().map(|v| convert_record_to_ipld(v)).collect())
        }
        other => other.clone(),
    }
}

/// CLI arguments for rsky-indexer
#[derive(Parser, Debug)]
#[command(name = "indexer")]
#[command(about = "rsky indexer - index AT Protocol records into PostgreSQL")]
struct Args {
    /// Index a specific repo by DID (one-off operation, bypasses Redis streams)
    #[arg(long)]
    index_repo: Option<String>,

    /// Index multiple repos from a CSV file (one DID per line, with optional header)
    #[arg(long)]
    index_repos_file: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "indexer=info,rsky_indexer=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse CLI arguments
    let args = Args::parse();

    // Check if we're in bulk repo indexing mode
    if let Some(csv_path) = args.index_repos_file {
        info!("Bulk repo indexing mode from file: {}", csv_path);
        return run_bulk_indexing(&csv_path).await;
    }

    // Check if we're in one-off repo indexing mode
    if let Some(did) = args.index_repo {
        info!("One-off repo indexing mode for DID: {}", did);
        return run_one_off_indexing(&did).await;
    }

    // Normal streaming indexer mode
    info!("Starting rsky-indexer");

    // Start metrics server
    let metrics_port = env::var("METRICS_PORT")
        .unwrap_or_else(|_| "9090".to_string())
        .parse::<u16>()
        .expect("METRICS_PORT must be a valid port number");

    tokio::spawn(async move {
        let metrics_route = warp::path!("metrics").map(|| match metrics::encode_metrics() {
            Ok(metrics) => warp::reply::with_status(metrics, warp::http::StatusCode::OK),
            Err(e) => {
                error!("Failed to encode metrics: {:?}", e);
                warp::reply::with_status(
                    format!("Error: {}", e),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
            }
        });

        info!("Metrics server starting on port {}", metrics_port);
        warp::serve(metrics_route)
            .run(([0, 0, 0, 0], metrics_port))
            .await;
    });

    // Load configuration from environment
    let config = load_config();

    info!("Configuration: {:?}", config);

    // Create PostgreSQL connection pool
    // Per CLAUDE.md: Conservative pool sizing to avoid connection exhaustion
    // Default: 20 connections max (not concurrency * 2 which can be 200!)
    let pool_max_size = env::var("DB_POOL_MAX_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    let _pool_min_idle = env::var("DB_POOL_MIN_IDLE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let mut pg_config = Config::new();
    pg_config.url = Some(config.database_url.clone());
    pg_config.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    pg_config.pool = Some(PoolConfig {
        max_size: pool_max_size,
        timeouts: Timeouts {
            wait: Some(Duration::from_secs(30)), // Wait max 30s for a connection
            create: Some(Duration::from_secs(30)), // Create connection max 30s
            recycle: Some(Duration::from_secs(30)), // Recycle connection max 30s
        },
        ..Default::default()
    });

    let pool = pg_config.create_pool(Some(Runtime::Tokio1), NoTls)?;

    info!(
        "PostgreSQL pool configured: max_size={}, concurrency={}",
        pool_max_size, config.concurrency
    );

    info!("Connected to PostgreSQL");

    // Create IdResolver for DID/handle resolution (optional)
    let id_resolver =
        if env::var("ENABLE_DID_RESOLUTION").unwrap_or_else(|_| "true".to_string()) == "true" {
            info!("DID resolution enabled");
            use tokio::sync::Mutex;
            let resolver_opts = rsky_identity::types::IdentityResolverOpts {
                timeout: None,
                plc_url: env::var("PLC_URL").ok(),
                did_cache: None,
                backup_nameservers: None,
            };
            Some(Arc::new(Mutex::new(rsky_identity::IdResolver::new(
                resolver_opts,
            ))))
        } else {
            info!("DID resolution disabled");
            None
        };

    // Create indexing service
    let indexing_service = Arc::new(IndexingService::new_with_resolver(
        pool.clone(),
        id_resolver,
    ));

    // Determine which indexers to run
    let mode = env::var("INDEXER_MODE").unwrap_or_else(|_| "all".to_string());

    match mode.as_str() {
        "stream" => {
            run_stream_indexers(config, indexing_service).await?;
        }
        "label" => {
            run_label_indexer(config, pool).await?;
        }
        "all" => {
            // Run all indexers concurrently
            let stream_config = config.clone();
            let label_config = config.clone();
            let stream_service = indexing_service.clone();
            let label_pool = pool.clone();

            tokio::select! {
                result = tokio::spawn(async move {
                    run_stream_indexers(stream_config, stream_service).await
                }) => {
                    error!("Stream indexer exited: {:?}", result);
                }
                result = tokio::spawn(async move {
                    run_label_indexer(label_config, label_pool).await
                }) => {
                    error!("Label indexer exited: {:?}", result);
                }
            }
        }
        _ => {
            error!("Unknown INDEXER_MODE: {}", mode);
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn run_stream_indexers(
    config: IndexerConfig,
    indexing_service: Arc<IndexingService>,
) -> Result<()> {
    info!(
        "Starting stream indexers for {} streams",
        config.streams.len()
    );

    // Create indexers for each stream
    let mut handles = Vec::new();

    let streams = config.streams.clone();
    for stream in streams {
        let mut stream_config = config.clone();
        stream_config.streams = vec![stream.clone()];

        let indexer = StreamIndexer::new(stream_config, indexing_service.clone()).await?;

        let handle = tokio::spawn(async move {
            if let Err(e) = indexer.run().await {
                error!("Stream indexer error for {}: {:?}", stream, e);
            }
        });

        handles.push(handle);
    }

    // Wait for all indexers
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

async fn run_label_indexer(config: IndexerConfig, pool: Pool) -> Result<()> {
    info!("Starting label indexer");

    let indexer = LabelIndexer::new(config, pool).await?;

    if let Err(e) = indexer.run().await {
        error!("Label indexer error: {:?}", e);
    }

    Ok(())
}

fn load_config() -> IndexerConfig {
    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/bsky".to_string());

    let streams_str = env::var("INDEXER_STREAMS")
        .unwrap_or_else(|_| format!("{},{}", streams::FIREHOSE_LIVE, streams::FIREHOSE_BACKFILL));

    let streams: Vec<String> = streams_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let consumer_group = env::var("INDEXER_GROUP").unwrap_or_else(|_| "firehose_group".to_string());

    let consumer_name = env::var("INDEXER_CONSUMER").unwrap_or_else(|_| "indexer_1".to_string());

    let concurrency = env::var("INDEXER_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let batch_size = env::var("INDEXER_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    IndexerConfig {
        redis_url,
        database_url,
        streams,
        consumer_group,
        consumer_name,
        concurrency,
        batch_size,
    }
}

/// One-off repo indexing: fetch repo CAR from PDS and write directly to PostgreSQL
async fn run_one_off_indexing(did: &str) -> Result<()> {
    use rsky_repo::car::read_car_with_root;
    use rsky_repo::parse::get_and_parse_record;
    use rsky_repo::readable_repo::ReadableRepo;
    use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tracing::warn;

    info!("Starting one-off repo indexing for DID: {}", did);

    // 1. Create PostgreSQL connection pool
    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/bsky".to_string());

    let mut pg_config = Config::new();
    pg_config.url = Some(database_url);
    pg_config.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    pg_config.pool = Some(PoolConfig {
        max_size: 10,
        timeouts: Timeouts {
            wait: Some(Duration::from_secs(30)),
            create: Some(Duration::from_secs(30)),
            recycle: Some(Duration::from_secs(30)),
        },
        ..Default::default()
    });

    let pool = pg_config.create_pool(Some(Runtime::Tokio1), NoTls)?;
    info!("Connected to PostgreSQL");

    // 2. Create IndexingService
    let indexing_service = Arc::new(IndexingService::new(pool));

    // 3. Resolve DID to get PDS endpoint
    info!("Resolving DID document for {}", did);
    let resolver_opts = rsky_identity::types::IdentityResolverOpts {
        timeout: None,
        plc_url: None,
        did_cache: None,
        backup_nameservers: None,
    };
    let mut resolver = rsky_identity::IdResolver::new(resolver_opts);
    let doc = resolver
        .did
        .resolve(did.to_string(), None)
        .await?
        .ok_or_else(|| anyhow::anyhow!("DID not found: {}", did))?;

    // Extract PDS endpoint
    let mut pds_endpoint = None;
    if let Some(services) = &doc.service {
        for service in services {
            if service.r#type == "AtprotoPersonalDataServer" || service.id == "#atproto_pds" {
                pds_endpoint = Some(service.service_endpoint.clone());
                break;
            }
        }
    }

    let pds_endpoint =
        pds_endpoint.ok_or_else(|| anyhow::anyhow!("No PDS endpoint found for DID: {}", did))?;

    info!("Resolved PDS endpoint: {}", pds_endpoint);

    // 4. Fetch repo CAR from PDS
    info!("Fetching repo CAR from {}", pds_endpoint);
    let url = format!("{}/xrpc/com.atproto.sync.getRepo?did={}", pds_endpoint, did);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Failed to fetch repo (status {}): {}",
            status,
            text
        ));
    }

    let car_bytes = response.bytes().await?.to_vec();
    info!("Downloaded {} bytes of CAR data", car_bytes.len());

    // 5. Parse CAR file
    info!("Parsing CAR file...");
    let car = read_car_with_root(car_bytes).await?;

    // 6. Verify and load repo
    info!("Loading repo...");
    let blockstore = MemoryBlockstore::new(Some(car.blocks.clone()))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create blockstore: {}", e))?;
    let storage = Arc::new(RwLock::new(blockstore));

    let mut repo = ReadableRepo::load(storage, car.root)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load repo: {}", e))?;

    // Verify DID matches
    if repo.did() != did {
        return Err(anyhow::anyhow!(
            "DID mismatch: expected {}, got {}",
            did,
            repo.did()
        ));
    }

    info!("Repo loaded successfully");

    // 7. Extract all records
    info!("Extracting records from repo...");
    let leaves = repo
        .data
        .list(None, None, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list records: {}", e))?;

    info!("Found {} records in repo", leaves.len());

    // Get block map from storage
    let storage_guard = repo.storage.read().await;
    let blocks_result = storage_guard
        .get_blocks(leaves.iter().map(|e| e.value).collect())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get blocks: {}", e))?;

    let _commit_cid = repo.cid.to_string();
    let rev = repo.commit.rev.clone();
    let now = chrono::Utc::now().to_rfc3339();

    // 8. Process records and write to PostgreSQL
    let mut indexed_count = 0;
    let mut skipped_count = 0;

    for entry in &leaves {
        // Parse key to get collection and rkey
        let parts: Vec<&str> = entry.key.split('/').collect();
        if parts.len() != 2 {
            warn!("Invalid data key: {}", entry.key);
            skipped_count += 1;
            continue;
        }
        let collection = parts[0].to_string();
        let rkey = parts[1].to_string();

        // Filter to only app.bsky.* and chat.bsky.* collections
        if !collection.starts_with("app.bsky.") && !collection.starts_with("chat.bsky.") {
            skipped_count += 1;
            continue;
        }

        // Get and parse record
        match get_and_parse_record(&blocks_result.blocks, entry.value) {
            Ok(parsed) => {
                let record_json_raw = serde_json::to_value(&parsed.record)?;
                let record_json = convert_record_to_ipld(&record_json_raw);

                let uri = format!("at://{}/{}/{}", did, collection, rkey);
                let cid = entry.value.to_string();

                // Index record using IndexingService
                if let Err(e) = indexing_service
                    .index_record(
                        &uri,
                        &cid,
                        &record_json,
                        rsky_indexer::indexing::WriteOpAction::Create,
                        &now,
                        &rev,
                        rsky_indexer::indexing::IndexingOptions::default(),
                    )
                    .await
                {
                    warn!("Failed to index record {}: {:?}", uri, e);
                } else {
                    indexed_count += 1;
                    if indexed_count % 100 == 0 {
                        info!("Indexed {} records...", indexed_count);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to parse record {}: {:?}", entry.value, e);
                skipped_count += 1;
            }
        }
    }

    info!("✅ One-off indexing complete for {}", did);
    info!("   Total records: {}", leaves.len());
    info!("   Indexed: {}", indexed_count);
    info!("   Skipped: {}", skipped_count);

    Ok(())
}

/// Bulk repo indexing: read DIDs from CSV file and index each one
async fn run_bulk_indexing(csv_path: &str) -> Result<()> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    info!("Starting bulk repo indexing from file: {}", csv_path);

    // Open and read CSV file
    let file = File::open(csv_path)?;
    let reader = BufReader::new(file);

    // Collect all DIDs (skip header line if present)
    let mut dids = Vec::new();
    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Skip header line (if first line contains "did" as text)
        if line_num == 0 && (line == "did" || line == "DID") {
            info!("Skipping CSV header line");
            continue;
        }

        // Validate DID format
        if !line.starts_with("did:") {
            warn!("Skipping invalid DID on line {}: {}", line_num + 1, line);
            continue;
        }

        dids.push(line.to_string());
    }

    info!("Loaded {} DIDs from CSV file", dids.len());

    if dids.is_empty() {
        return Err(anyhow::anyhow!("No valid DIDs found in CSV file"));
    }

    // Process each DID
    let mut success_count = 0;
    let mut failure_count = 0;
    let start_time = std::time::Instant::now();

    for (idx, did) in dids.iter().enumerate() {
        let progress = idx + 1;
        info!("Processing DID {}/{}: {}", progress, dids.len(), did);

        match run_one_off_indexing(did).await {
            Ok(()) => {
                success_count += 1;
                info!("✅ Successfully indexed {}/{}", progress, dids.len());
            }
            Err(e) => {
                failure_count += 1;
                error!("❌ Failed to index {} ({}/{}): {:?}", did, progress, dids.len(), e);
            }
        }

        // Progress report every 10 DIDs
        if progress % 10 == 0 || progress == dids.len() {
            let elapsed = start_time.elapsed().as_secs();
            let rate = if elapsed > 0 {
                progress as f64 / elapsed as f64
            } else {
                0.0
            };
            info!(
                "Progress: {}/{} ({:.1}%) - Success: {} - Failed: {} - Rate: {:.2} DIDs/sec",
                progress,
                dids.len(),
                (progress as f64 / dids.len() as f64) * 100.0,
                success_count,
                failure_count,
                rate
            );
        }
    }

    let total_time = start_time.elapsed();
    info!("========================================");
    info!("Bulk indexing complete!");
    info!("Total DIDs processed: {}", dids.len());
    info!("✅ Successful: {}", success_count);
    info!("❌ Failed: {}", failure_count);
    info!("⏱️  Total time: {:.2?}", total_time);
    info!(
        "📊 Average rate: {:.2} DIDs/sec",
        dids.len() as f64 / total_time.as_secs_f64()
    );
    info!("========================================");

    if failure_count > 0 {
        return Err(anyhow::anyhow!(
            "{} out of {} DIDs failed to index",
            failure_count,
            dids.len()
        ));
    }

    Ok(())
}
