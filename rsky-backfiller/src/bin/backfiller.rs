use rsky_backfiller::metrics;
use rsky_backfiller::repo_backfiller::RepoBackfiller;
use rsky_backfiller::BackfillerConfig;
use std::env;
use tokio::signal;
use tracing::{error, info};
use warp::Filter;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            env::var("RUST_LOG").unwrap_or_else(|_| "info,rsky_backfiller=debug".to_string()),
        )
        .init();

    info!("Starting rsky-backfiller");

    // Initialize metrics (ensures all metrics are registered with Prometheus)
    metrics::initialize_metrics();
    info!("Metrics initialized");

    // Load configuration from environment
    let config = BackfillerConfig {
        redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        stream_in: env::var("BACKFILLER_BACKFILL_STREAM")
            .unwrap_or_else(|_| "repo_backfill".to_string()),
        stream_out: env::var("BACKFILLER_FIREHOSE_STREAM")
            .unwrap_or_else(|_| "firehose_backfill".to_string()),
        stream_dead_letter: env::var("BACKFILLER_DLQ_STREAM")
            .unwrap_or_else(|_| "repo_backfill_dlq".to_string()),
        consumer_group: env::var("BACKFILLER_GROUP")
            .unwrap_or_else(|_| "repo_backfill_group".to_string()),
        consumer_name: env::var("BACKFILLER_CONSUMER").expect("BACKFILLER_CONSUMER must be set"),
        concurrency: env::var("BACKFILLER_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50), // Increased from 20 to 50 for better throughput
        high_water_mark: env::var("BACKFILLER_FIREHOSE_STREAM_HIGH_WATER_MARK")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(500_000), // Increased from 100k to 500k for better buffering
        http_timeout_secs: env::var("BACKFILLER_HTTP_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60),
        max_retries: env::var("BACKFILLER_MAX_RETRIES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3),
        retry_initial_backoff_ms: env::var("BACKFILLER_RETRY_INITIAL_BACKOFF_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000),
        retry_max_backoff_ms: env::var("BACKFILLER_RETRY_MAX_BACKOFF_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30000),
        metrics_port: env::var("BACKFILLER_METRICS_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(9090),
    };

    info!("Configuration:");
    info!("  Redis URL: {}", config.redis_url);
    info!("  Stream In: {}", config.stream_in);
    info!("  Stream Out: {}", config.stream_out);
    info!("  Stream DLQ: {}", config.stream_dead_letter);
    info!("  Consumer Group: {}", config.consumer_group);
    info!("  Consumer Name: {}", config.consumer_name);
    info!("  Concurrency: {}", config.concurrency);
    info!("  High Water Mark: {}", config.high_water_mark);
    info!("  HTTP Timeout: {}s", config.http_timeout_secs);
    info!("  Max Retries: {}", config.max_retries);
    info!(
        "  Retry Initial Backoff: {}ms",
        config.retry_initial_backoff_ms
    );
    info!("  Retry Max Backoff: {}ms", config.retry_max_backoff_ms);
    info!("  Metrics Port: {}", config.metrics_port);

    // Start metrics server
    let metrics_port = config.metrics_port;
    tokio::spawn(async move {
        let metrics_route = warp::path("metrics").map(|| match metrics::encode_metrics() {
            Ok(body) => warp::http::Response::builder()
                .header("Content-Type", "text/plain; version=0.0.4")
                .body(body)
                .unwrap(),
            Err(e) => warp::http::Response::builder()
                .status(500)
                .body(format!("Error encoding metrics: {}", e))
                .unwrap(),
        });

        info!("Metrics server listening on 0.0.0.0:{}", metrics_port);
        warp::serve(metrics_route)
            .run(([0, 0, 0, 0], metrics_port))
            .await;
    });

    // Create backfiller
    let backfiller = match RepoBackfiller::new(config) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to create backfiller: {:?}", e);
            std::process::exit(1);
        }
    };

    // Setup graceful shutdown
    let shutdown = async {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Received shutdown signal");
            }
            Err(err) => {
                error!("Error waiting for shutdown signal: {}", err);
            }
        }
    };

    // Run backfiller with graceful shutdown
    tokio::select! {
        result = backfiller.run() => {
            if let Err(e) = result {
                error!("Backfiller error: {:?}", e);
                std::process::exit(1);
            }
        }
        _ = shutdown => {
            info!("Shutting down gracefully...");
            // Give in-flight tasks time to complete
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }
}
