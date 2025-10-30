use anyhow::Result;
use deadpool_postgres::{Config, ManagerConfig, Pool, PoolConfig, RecyclingMethod, Runtime, Timeouts};
use rsky_indexer::{
    indexing::IndexingService, label_indexer::LabelIndexer, stream_indexer::StreamIndexer, streams,
    IndexerConfig,
};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio_postgres::NoTls;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

    info!("Starting rsky-indexer");

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
            wait: Some(Duration::from_secs(30)),        // Wait max 30s for a connection
            create: Some(Duration::from_secs(30)),      // Create connection max 30s
            recycle: Some(Duration::from_secs(30)),     // Recycle connection max 30s
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
