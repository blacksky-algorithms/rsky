use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use color_eyre::Result;
use mimalloc::MiMalloc;

mod api;
mod bloom;
mod bulk_load;
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

    #[clap(long, env = "RELAY_HOST", default_value = "wss://atproto.africa")]
    relay_host: String,

    #[clap(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Path to a CSV (creator,subjectDid) produced by `\copy`. When set, this
    /// is preferred over a live PG load -- see plan recursive-honking-sprout.
    #[clap(long, env = "GRAPH_LOAD_FROM_FILE")]
    load_from_file: Option<String>,

    /// Bearer token gating POST /admin/bulk-load. Unset = admin disabled.
    #[clap(long, env = "GRAPH_ADMIN_TOKEN")]
    admin_token: Option<String>,
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

    let shutdown_flag = Arc::new(AtomicBool::new(false));
    {
        let sf = Arc::clone(&shutdown_flag);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutdown signal received");
            sf.store(true, Ordering::Relaxed);
            SHUTDOWN.store(true, Ordering::Relaxed);
        });
    }

    // Exponential save backoff: 60s, 120s, 240s, ... capped at 30 min.
    {
        let sf_persist = Arc::clone(&shutdown_flag);
        let db_path_persist = args.db_path.clone();
        let graph_persist = Arc::clone(&graph);
        tokio::spawn(async move {
            let mut delay_secs: u64 = 60;
            const MAX_DELAY: u64 = 1800;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                if sf_persist.load(Ordering::Relaxed) {
                    break;
                }
                let start = std::time::Instant::now();
                let users = graph_persist.user_count();
                match persistence::save_to_lmdb(&db_path_persist, &graph_persist).await {
                    Ok(()) => {
                        let elapsed = start.elapsed().as_secs_f64();
                        tracing::info!(
                            "periodic LMDB save: {} users in {:.1}s (next in {}s)",
                            users,
                            elapsed,
                            delay_secs
                        );
                    }
                    Err(e) => tracing::error!("periodic LMDB save failed: {e}"),
                }
                // Back off so save overhead stays a small fraction of bulk-load time.
                delay_secs = (delay_secs.saturating_mul(2)).min(MAX_DELAY);
            }
        });
    }

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

    // Start firehose sync in background
    let graph_firehose = Arc::clone(&graph);
    let relay_host = args.relay_host.clone();
    let sf_firehose = Arc::clone(&shutdown_flag);
    tokio::spawn(async move {
        firehose::tail_firehose(&relay_host, &graph_firehose, &sf_firehose).await;
    });

    // Build admin state. The token gates POST /admin/bulk-load; without it
    // the route always 401s. database_url is reused from the startup arg.
    let admin = Arc::new(api::AdminState::new(
        args.admin_token.clone(),
        args.database_url.clone(),
    ));

    // Start HTTP API (blocks until shutdown)
    api::serve(args.port, Arc::clone(&graph), admin, shutdown_flag).await?;

    // Final persist on shutdown
    tracing::info!("final LMDB save");
    persistence::save_to_lmdb(&args.db_path, &graph).await.ok();

    tracing::info!("goodbye");
    Ok(())
}
