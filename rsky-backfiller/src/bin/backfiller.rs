use rsky_backfiller::repo_backfiller::RepoBackfiller;
use rsky_backfiller::BackfillerConfig;
use std::env;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            env::var("RUST_LOG").unwrap_or_else(|_| "info,rsky_backfiller=debug".to_string()),
        )
        .init();

    info!("Starting rsky-backfiller");

    // Load configuration from environment
    let config = BackfillerConfig {
        redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        stream_in: env::var("BACKFILLER_BACKFILL_STREAM")
            .unwrap_or_else(|_| "repo_backfill".to_string()),
        stream_out: env::var("BACKFILLER_FIREHOSE_STREAM")
            .unwrap_or_else(|_| "firehose_backfill".to_string()),
        consumer_group: env::var("BACKFILLER_GROUP")
            .unwrap_or_else(|_| "repo_backfill_group".to_string()),
        consumer_name: env::var("BACKFILLER_CONSUMER").expect("BACKFILLER_CONSUMER must be set"),
        concurrency: env::var("BACKFILLER_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2),
        high_water_mark: env::var("BACKFILLER_FIREHOSE_STREAM_HIGH_WATER_MARK")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100_000),
    };

    info!("Configuration:");
    info!("  Redis URL: {}", config.redis_url);
    info!("  Stream In: {}", config.stream_in);
    info!("  Stream Out: {}", config.stream_out);
    info!("  Consumer Group: {}", config.consumer_group);
    info!("  Consumer Name: {}", config.consumer_name);
    info!("  Concurrency: {}", config.concurrency);
    info!("  High Water Mark: {}", config.high_water_mark);

    // Create backfiller
    let backfiller = match RepoBackfiller::new(config) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to create backfiller: {:?}", e);
            std::process::exit(1);
        }
    };

    // Run backfiller
    if let Err(e) = backfiller.run().await {
        error!("Backfiller error: {:?}", e);
        std::process::exit(1);
    }
}
