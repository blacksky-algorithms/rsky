use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use color_eyre::Result;
use mimalloc::MiMalloc;

mod api;
mod bloom;
mod bulk_load;
mod config;
mod firehose;
mod graph;
mod metrics;
mod persistence;
mod types;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Parser)]
#[command(name = "rsky-graph")]
#[command(about = "AT Protocol follow graph service using roaring bitmaps and bloom filters")]
struct Args {
    #[clap(long, env = "GRAPH_PORT", default_value = "3890")]
    port: u16,

    #[clap(long, env = "GRAPH_DB_PATH", default_value = "/data/graph")]
    db_path: String,

    #[clap(long, env = "RELAY_HOST", default_value = "wss://bsky.network")]
    relay_host: String,

    #[clap(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Path to a CSV (creator,subjectDid) produced by `\copy`. When set, this
    /// is preferred over a live PG load -- see plan recursive-honking-sprout.
    #[clap(long, env = "GRAPH_LOAD_FROM_FILE")]
    load_from_file: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    color_eyre::install()?;

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    let args = Args::parse();

    tracing::info!("starting rsky-graph");
    tracing::info!("port: {}", args.port);
    tracing::info!("db_path: {}", args.db_path);
    tracing::info!("relay: {}", args.relay_host);

    // Initialize graph
    let graph = Arc::new(graph::FollowGraph::new());

    // Load from LMDB if available
    let loaded = persistence::load_from_lmdb(&args.db_path, &graph).await;
    match loaded {
        Ok(count) if count > 0 => {
            tracing::info!("loaded {} users from LMDB", count);
        }
        Ok(_) => {
            // Empty LMDB -- prefer file load (zero PG transaction) over a live PG load.
            if let Some(ref path) = args.load_from_file {
                tracing::info!("LMDB empty, bulk-loading from file: {path}");
                bulk_load::bulk_load_from_file(std::path::Path::new(path), &graph).await?;
                tracing::info!(
                    "bulk load complete: {} users, {} follows",
                    graph.user_count(),
                    graph.follow_count()
                );
                bloom::build_all_bloom_filters(&graph);
                tracing::info!("bloom filters built");
                persistence::save_to_lmdb(&args.db_path, &graph).await?;
                tracing::info!("persisted to LMDB");
            } else if let Some(ref db_url) = args.database_url {
                tracing::info!("LMDB empty, starting keyset bulk load from PostgreSQL");
                bulk_load::bulk_load_keyset(db_url, &graph).await?;
                tracing::info!(
                    "bulk load complete: {} users, {} follows",
                    graph.user_count(),
                    graph.follow_count()
                );
                bloom::build_all_bloom_filters(&graph);
                tracing::info!("bloom filters built");
                persistence::save_to_lmdb(&args.db_path, &graph).await?;
                tracing::info!("persisted to LMDB");
            } else {
                tracing::warn!(
                    "LMDB empty and no GRAPH_LOAD_FROM_FILE / DATABASE_URL -- starting with empty graph"
                );
            }
        }
        Err(e) => {
            tracing::warn!("failed to load LMDB: {e}, starting fresh");
        }
    }

    // Register shutdown handler
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let sf = Arc::clone(&shutdown_flag);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("shutdown signal received");
        sf.store(true, Ordering::Relaxed);
        SHUTDOWN.store(true, Ordering::Relaxed);
    });

    // Start firehose sync in background
    let graph_firehose = Arc::clone(&graph);
    let relay_host = args.relay_host.clone();
    let db_path_persist = args.db_path.clone();
    let graph_persist = Arc::clone(&graph);
    let sf_firehose = Arc::clone(&shutdown_flag);

    tokio::spawn(async move {
        firehose::tail_firehose(&relay_host, &graph_firehose, &sf_firehose).await;
    });

    // Periodic LMDB persistence
    let sf_persist = Arc::clone(&shutdown_flag);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if sf_persist.load(Ordering::Relaxed) {
                break;
            }
            if let Err(e) = persistence::save_to_lmdb(&db_path_persist, &graph_persist).await {
                tracing::error!("periodic LMDB save failed: {e}");
            }
        }
    });

    // Start HTTP API (blocks until shutdown)
    api::serve(args.port, Arc::clone(&graph), shutdown_flag).await?;

    // Final persist on shutdown
    tracing::info!("final LMDB save");
    persistence::save_to_lmdb(&args.db_path, &graph).await.ok();

    tracing::info!("goodbye");
    Ok(())
}
