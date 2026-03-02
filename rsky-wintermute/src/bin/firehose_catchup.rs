use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clap::Parser;
use color_eyre::Result;
use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
use futures::StreamExt;
use tokio::sync::Semaphore;
use tokio_postgres::NoTls;
use tokio_tungstenite::tungstenite::Message;

use rsky_wintermute::indexer::IndexerManager;
use rsky_wintermute::ingester::IngesterManager;

#[derive(Debug, Parser)]
#[command(name = "firehose_catchup")]
#[command(about = "Replay a range of firehose events to fill indexing gaps")]
struct Args {
    /// Starting cursor (sequence number to replay from)
    #[arg(long)]
    start_cursor: i64,

    /// Ending cursor (stop after reaching this sequence number)
    #[arg(long)]
    end_cursor: i64,

    /// PostgreSQL connection URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// Firehose relay host
    #[arg(long, default_value = "bsky.network")]
    relay_host: String,

    /// Maximum concurrent indexing tasks
    #[arg(long, default_value = "200")]
    concurrency: usize,

    /// Database pool size
    #[arg(long, default_value = "40")]
    pool_size: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install rustls crypto provider before any TLS operations
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    if args.start_cursor >= args.end_cursor {
        return Err(color_eyre::eyre::eyre!(
            "start_cursor ({}) must be less than end_cursor ({})",
            args.start_cursor,
            args.end_cursor
        ));
    }

    let total_events = args.end_cursor - args.start_cursor;
    tracing::info!(
        "firehose catchup: replaying {} events from cursor {} to {}",
        total_events,
        args.start_cursor,
        args.end_cursor
    );

    // Setup database pool
    let mut cfg = Config::new();
    cfg.url = Some(args.database_url);
    cfg.pool = Some(deadpool_postgres::PoolConfig {
        max_size: args.pool_size,
        ..Default::default()
    });
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });

    let pool = Arc::new(cfg.create_pool(Some(Runtime::Tokio1), NoTls)?);

    // Test DB connection
    let test_client = pool.get().await?;
    drop(test_client);
    tracing::info!("database connection OK, pool_size={}", args.pool_size);

    let semaphore = Arc::new(Semaphore::new(args.concurrency));
    let processed = Arc::new(AtomicU64::new(0));
    let last_seq = Arc::new(AtomicI64::new(0));
    let end_cursor = args.end_cursor;

    // Connect to firehose
    let url = format!(
        "wss://{}/xrpc/com.atproto.sync.subscribeRepos?cursor={}",
        args.relay_host, args.start_cursor
    );
    tracing::info!("connecting to {url}");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await?;
    let (_write, mut read) = ws_stream.split();

    tracing::info!("connected, processing events...");

    let start_time = Instant::now();
    let mut last_log_time = Instant::now();
    let mut last_log_count = 0u64;

    loop {
        let msg = tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => {
                        tracing::error!("websocket error: {e}");
                        break;
                    }
                    None => {
                        tracing::info!("websocket stream ended");
                        break;
                    }
                }
            }
            () = tokio::time::sleep(Duration::from_secs(30)) => {
                tracing::warn!("no message in 30s, connection may be dead");
                break;
            }
        };

        if let Message::Binary(data) = msg {
            let event = match IngesterManager::parse_message(&data) {
                Ok(rsky_wintermute::ingester::ParseResult::Event(e)) => e,
                Ok(rsky_wintermute::ingester::ParseResult::OutdatedCursor) => {
                    tracing::warn!("cursor too old, relay will resume from oldest available");
                    continue;
                }
                Ok(_) => continue,
                Err(e) => {
                    tracing::warn!("failed to parse message: {e}");
                    continue;
                }
            };

            let seq = event.seq;

            // Stop if we've reached the end cursor
            if seq >= end_cursor {
                tracing::info!("reached end cursor {end_cursor} at seq {seq}, stopping");
                break;
            }

            last_seq.store(seq, Ordering::Relaxed);

            // Skip identity/account/sync events - they're ephemeral and the main
            // instance handles current state. We only need commit events for records.
            if event.kind != "commit" {
                continue;
            }

            // Parse and index
            match IngesterManager::parse_event_to_jobs(&event).await {
                Ok(jobs) => {
                    for job in jobs {
                        let permit = semaphore.clone().acquire_owned().await?;
                        let pool_clone = Arc::clone(&pool);
                        let processed_clone = Arc::clone(&processed);
                        tokio::spawn(async move {
                            if let Err(e) = IndexerManager::process_job(&pool_clone, &job).await {
                                tracing::debug!("indexing failed for {}: {e}", job.uri);
                            }
                            processed_clone.fetch_add(1, Ordering::Relaxed);
                            drop(permit);
                        });
                    }
                }
                Err(e) => {
                    tracing::debug!("failed to parse event seq={seq}: {e}");
                }
            }

            // Progress logging every 10 seconds
            if last_log_time.elapsed() > Duration::from_secs(10) {
                let current_processed = processed.load(Ordering::Relaxed);
                let current_seq = last_seq.load(Ordering::Relaxed);
                let rate =
                    (current_processed - last_log_count) as f64 / last_log_time.elapsed().as_secs_f64();
                let remaining = end_cursor - current_seq;
                let pct = ((current_seq - args.start_cursor) as f64 / total_events as f64) * 100.0;
                tracing::info!(
                    "progress: {pct:.1}% | seq={current_seq} | processed={current_processed} | rate={rate:.0}/s | remaining={remaining}"
                );
                last_log_count = current_processed;
                last_log_time = Instant::now();
            }
        }
    }

    // Wait for in-flight tasks to complete
    tracing::info!("waiting for in-flight tasks to complete...");
    let _ = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if semaphore.available_permits() == args.concurrency {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;

    let final_processed = processed.load(Ordering::Relaxed);
    let final_seq = last_seq.load(Ordering::Relaxed);
    let elapsed = start_time.elapsed();
    let rate = final_processed as f64 / elapsed.as_secs_f64();

    tracing::info!(
        "catchup complete: processed={final_processed} records, final_seq={final_seq}, elapsed={:.1}s, avg_rate={rate:.0}/s",
        elapsed.as_secs_f64()
    );

    Ok(())
}
