use clap::Parser;
use color_eyre::Result;
use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
use futures::SinkExt;
use futures::stream::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::interval;
use tokio_postgres::NoTls;
use tokio_tungstenite::tungstenite::Message;

use rsky_wintermute::ingester::labels::parse_label_message;

#[derive(Debug, Parser)]
#[command(name = "label_sync")]
#[command(
    about = "Replay label streams from cursor=0 to apply missed negations.\n\
             Only applies negations to existing labels. Does NOT insert new labels.\n\
             Safe to run alongside live wintermute - won't recreate deleted labels."
)]
struct Args {
    /// Labeler hosts to sync (comma-separated)
    #[arg(long, env = "LABELER_HOSTS", value_delimiter = ',')]
    labeler_hosts: Vec<String>,

    /// PostgreSQL connection URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// Number of concurrent indexing tasks
    #[arg(long, default_value = "8")]
    concurrency: usize,

    /// Stop after reaching this cursor value (0 = run until caught up)
    #[arg(long, default_value = "0")]
    stop_at_cursor: i64,
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let hosts: Vec<String> = args
        .labeler_hosts
        .into_iter()
        .filter(|h| !h.is_empty())
        .collect();

    if hosts.is_empty() {
        eprintln!("No labeler hosts provided. Use --labeler-hosts or LABELER_HOSTS env var.");
        return Ok(());
    }

    let mut pg_config = Config::new();
    pg_config.url = Some(args.database_url);
    pg_config.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    pg_config.pool = Some(deadpool_postgres::PoolConfig::new(16));

    let pool = Arc::new(
        pg_config
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| color_eyre::eyre::eyre!("pool creation failed: {e}"))?,
    );

    let semaphore = Arc::new(Semaphore::new(args.concurrency));

    let mut tasks = Vec::new();
    for host in hosts {
        let pool = Arc::clone(&pool);
        let semaphore = Arc::clone(&semaphore);
        let stop_at = args.stop_at_cursor;
        tasks.push(tokio::spawn(async move {
            if let Err(e) = sync_labeler(&host, &pool, &semaphore, stop_at).await {
                tracing::error!("label sync failed for {host}: {e}");
            }
        }));
    }

    for task in tasks {
        task.await?;
    }

    println!("\nLabel sync complete.");
    Ok(())
}

async fn sync_labeler(
    labeler_host: &str,
    pool: &Arc<deadpool_postgres::Pool>,
    semaphore: &Arc<Semaphore>,
    stop_at_cursor: i64,
) -> Result<()> {
    let clean_hostname = labeler_host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');

    // Always start from cursor 0 for full replay
    let url_str = format!("wss://{clean_hostname}/xrpc/com.atproto.label.subscribeLabels?cursor=0");
    let url = url::Url::parse(&url_str).map_err(|e| color_eyre::eyre::eyre!("invalid url: {e}"))?;

    println!("Connecting to {url} (replay from beginning)...");

    let (ws_stream, _) = tokio_tungstenite::connect_async(url.as_str()).await?;
    let (mut write, mut read) = ws_stream.split();

    let ping_task = tokio::spawn(async move {
        let mut ping_interval = interval(Duration::from_secs(30));
        loop {
            ping_interval.tick().await;
            if write.send(Message::Ping(vec![])).await.is_err() {
                break;
            }
        }
    });

    let mut total_events = 0u64;
    let mut total_negations_applied = 0u64;
    let mut total_skipped = 0u64;
    let mut last_seq = 0i64;
    let start = std::time::Instant::now();

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
                        println!("Stream ended at seq={last_seq}");
                        break;
                    }
                }
            }
            () = tokio::time::sleep(Duration::from_millis(100)) => continue,
        };

        if let Message::Binary(data) = msg {
            match parse_label_message(&data) {
                Ok(Some(label_event)) => {
                    last_seq = label_event.seq;
                    total_events += 1;

                    // Stop if we've reached the target cursor
                    if stop_at_cursor > 0 && label_event.seq >= stop_at_cursor {
                        println!("Reached target cursor {stop_at_cursor}, stopping.");
                        break;
                    }

                    let permit = semaphore.clone().acquire_owned().await?;
                    let pool_clone = Arc::clone(pool);
                    let event = label_event.clone();
                    let (neg_count, skip_count) = tokio::spawn(async move {
                        let result = process_label_event_safe(&pool_clone, &event).await;
                        drop(permit);
                        result.unwrap_or((0, 0))
                    })
                    .await?;

                    total_negations_applied += neg_count;
                    total_skipped += skip_count;

                    if total_events % 10000 == 0 {
                        let elapsed = start.elapsed().as_secs();
                        let rate = if elapsed > 0 {
                            total_events / elapsed
                        } else {
                            0
                        };
                        println!(
                            "[{labeler_host}] seq={last_seq} events={total_events} \
                             negations={total_negations_applied} skipped={total_skipped} \
                             rate={rate}/s"
                        );
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::debug!("parse error: {e}");
                }
            }
        }
    }

    ping_task.abort();

    println!(
        "\n=== {labeler_host} sync complete ===\n\
         Events processed: {total_events}\n\
         Negations applied: {total_negations_applied}\n\
         Skipped (non-negation): {total_skipped}\n\
         Final seq: {last_seq}\n\
         Duration: {:.1}s",
        start.elapsed().as_secs_f64()
    );

    Ok(())
}

/// Process a label event: only apply negations to existing labels.
/// Skips all positive labels entirely -- never inserts new rows.
/// Returns (negations_applied, skipped).
async fn process_label_event_safe(
    pool: &deadpool_postgres::Pool,
    event: &rsky_wintermute::types::LabelEvent,
) -> Result<(u64, u64)> {
    let client = pool.get().await?;
    let mut negations = 0u64;
    let mut skipped = 0u64;

    for label in &event.labels {
        if !label.neg {
            skipped += 1;
            continue;
        }

        // Apply negation to ALL existing labels with matching (src, uri, val)
        let negated = client
            .execute(
                "UPDATE label SET neg = true
                 WHERE src = $1 AND uri = $2 AND val = $3 AND neg = false",
                &[&label.src, &label.uri, &label.val],
            )
            .await;

        if let Ok(count) = negated {
            negations += count;
            if count > 0 {
                tracing::info!(
                    "negated {count} label(s): src={} uri={} val={}",
                    label.src,
                    label.uri,
                    label.val
                );
            }
        }
    }

    Ok((negations, skipped))
}
