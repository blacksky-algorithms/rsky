use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use color_eyre::Result;
use mimalloc::MiMalloc;

use rsky_graph::{api, bulk_load, firehose, graph, persistence, types};

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
    let load_state = Arc::new(types::LoadState::new());

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

    // Load existing state from LMDB synchronously before any concurrent task
    // starts (a second env handle on the same path during load races and
    // corrupts state).
    match persistence::load_from_lmdb(&args.db_path, &graph).await {
        Ok(count) if count > 0 => {
            tracing::info!("loaded {} users from LMDB", count);
            // Existing on-disk state is from a previous fully-completed bulk-load,
            // so the API can serve all queries.
            load_state.mark_complete();
        }
        Ok(_) => {
            tracing::info!(
                "LMDB empty -- API will return 503 for unloaded actors until bulk-load progresses"
            );
        }
        Err(e) => {
            tracing::warn!("failed to load LMDB: {e}, starting fresh");
        }
    }

    // Background bulk-load -- the API starts serving immediately and answers
    // 503 for actors above the load cursor until they're processed.
    if !load_state.is_complete() {
        if let Some(ref path) = args.load_from_file {
            let path = path.clone();
            let graph_load = Arc::clone(&graph);
            let load_state_bg = Arc::clone(&load_state);
            let db_path_save = args.db_path.clone();
            tokio::spawn(async move {
                tracing::info!("background bulk-load from file: {path}");
                if let Err(e) =
                    bulk_load::bulk_load_from_file(std::path::Path::new(&path), &graph_load).await
                {
                    tracing::error!("bulk load from file failed: {e}");
                    return;
                }
                load_state_bg.mark_complete();
                post_load_finalize(&db_path_save, &graph_load).await;
            });
        } else if let Some(ref db_url) = args.database_url {
            let db_url = db_url.clone();
            let db_path_save = args.db_path.clone();
            let graph_load = Arc::clone(&graph);
            let load_state_bg = Arc::clone(&load_state);
            tokio::spawn(async move {
                tracing::info!("background keyset bulk load from PostgreSQL");
                if let Err(e) =
                    bulk_load::bulk_load_keyset(&db_url, &graph_load, &load_state_bg).await
                {
                    tracing::error!("bulk load failed: {e}");
                    return;
                }
                post_load_finalize(&db_path_save, &graph_load).await;
            });
        } else {
            tracing::warn!(
                "LMDB empty and no GRAPH_LOAD_FROM_FILE / DATABASE_URL -- API serves an empty graph"
            );
            load_state.mark_complete();
        }
    }

    // Start firehose sync in background
    let graph_firehose = Arc::clone(&graph);
    let relay_host = args.relay_host.clone();
    let sf_firehose = Arc::clone(&shutdown_flag);
    tokio::spawn(async move {
        firehose::tail_firehose(&relay_host, &graph_firehose, &sf_firehose).await;
    });

    // Periodic save. The save itself iterates DashMap (read locks per shard);
    // bulk-load + firehose only mutate DashMap. Single env opened per save call.
    // Sequential schedule: each save runs to completion before the next sleep.
    {
        let sf_persist = Arc::clone(&shutdown_flag);
        let db_path_persist = args.db_path.clone();
        let graph_persist = Arc::clone(&graph);
        let load_state_persist = Arc::clone(&load_state);
        tokio::spawn(async move {
            // Save more often during bulk-load so a crash loses minutes, not hours.
            // After load completes we drop to a slower steady-state cadence.
            loop {
                let active = !load_state_persist.is_complete();
                let next_sleep = if active { 600 } else { 1800 };
                tokio::time::sleep(std::time::Duration::from_secs(next_sleep)).await;
                if sf_persist.load(Ordering::Relaxed) {
                    break;
                }
                let users = graph_persist.user_count();
                let start = std::time::Instant::now();
                match persistence::save_to_lmdb(&db_path_persist, &graph_persist).await {
                    Ok(()) => tracing::info!(
                        "periodic LMDB save: {users} users in {:.1}s",
                        start.elapsed().as_secs_f64()
                    ),
                    Err(e) => tracing::error!("periodic LMDB save failed: {e}"),
                }
            }
        });
    }

    // Build admin state. The token gates POST /admin/bulk-load; without it
    // the route always 401s. database_url is reused from the startup arg.
    let admin = Arc::new(api::AdminState::new(
        args.admin_token.clone(),
        args.database_url.clone(),
    ));

    // Start HTTP API. Serving happens immediately even if bulk-load is still
    // running -- handlers consult load_state to decide whether to 503.
    api::serve(
        args.port,
        Arc::clone(&graph),
        admin,
        Arc::clone(&load_state),
        shutdown_flag,
    )
    .await?;

    // Final persist on shutdown
    tracing::info!("final LMDB save");
    persistence::save_to_lmdb(&args.db_path, &graph).await.ok();

    tracing::info!("goodbye");
    Ok(())
}

/// Post-bulk-load: persist the graph to LMDB so the next restart can serve
/// from disk without re-loading from PG. Bloom filters are rebuilt lazily on
/// first query rather than upfront -- a 25M-user upfront rebuild blocks the
/// API for hours.
async fn post_load_finalize(db_path: &str, graph: &graph::FollowGraph) {
    tracing::info!(
        "bulk load complete: {} users, {} follows",
        graph.user_count(),
        graph.follow_count()
    );
    if let Err(e) = persistence::save_to_lmdb(db_path, graph).await {
        tracing::error!("post-load LMDB save failed: {e}");
    } else {
        tracing::info!("post-load LMDB save done");
    }
}
