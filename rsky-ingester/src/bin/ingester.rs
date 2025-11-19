use anyhow::Result;
use rsky_ingester::{
    backfill::BackfillIngester, firehose::FirehoseIngester, labeler::LabelerIngester, metrics,
    IngesterConfig,
};
use std::env;
use tokio::time::Duration;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use warp::Filter;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ingester=info,rsky_ingester=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Set up panic hook for better debugging
    // Use eprintln! instead of error! to ensure output even if logging is broken
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.as_str()
        } else {
            "Unknown panic payload"
        };

        let location = if let Some(location) = panic_info.location() {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        } else {
            "unknown location".to_string()
        };

        eprintln!(
            "\n================================================================================"
        );
        eprintln!("FATAL PANIC OCCURRED");
        eprintln!(
            "================================================================================"
        );
        eprintln!("Location: {}", location);
        eprintln!("Message: {}", message);
        eprintln!("\nThis is a critical error that should never occur in production.");
        eprintln!("Please report this issue with the complete log output.");
        eprintln!(
            "================================================================================\n"
        );
        eprintln!(
            "Backtrace:\n{:?}",
            std::backtrace::Backtrace::force_capture()
        );
        eprintln!(
            "================================================================================\n"
        );
    }));

    info!("Starting rsky-ingester");

    // Initialize metrics (ensures all metrics are registered with Prometheus)
    metrics::initialize_metrics();
    info!("Metrics initialized");

    // Start metrics server
    let metrics_port = env::var("METRICS_PORT")
        .unwrap_or_else(|_| "4100".to_string())
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

    // Determine which ingesters to run
    let mode = env::var("INGESTER_MODE").unwrap_or_else(|_| "all".to_string());

    match mode.as_str() {
        "firehose" => {
            eprintln!("[EXIT-TRACE] About to run firehose mode");
            run_firehose(config).await?;
            eprintln!("[EXIT-TRACE] Firehose mode exited - EXIT PATH A");
            error!("[EXIT-TRACE] Firehose mode exited - EXIT PATH A");
            return Err(anyhow::anyhow!(
                "CRITICAL: Firehose mode exited unexpectedly"
            ));
        }
        "backfill" => {
            eprintln!("[EXIT-TRACE] About to run backfill mode");
            run_backfill(config).await?;
            eprintln!("[EXIT-TRACE] Backfill mode exited - EXIT PATH B");
            error!("[EXIT-TRACE] Backfill mode exited - EXIT PATH B");
            return Err(anyhow::anyhow!(
                "CRITICAL: Backfill mode exited unexpectedly"
            ));
        }
        "labeler" => {
            eprintln!("[EXIT-TRACE] About to run labeler mode");
            run_labeler(config).await?;
            eprintln!("[EXIT-TRACE] Labeler mode exited - EXIT PATH C");
            error!("[EXIT-TRACE] Labeler mode exited - EXIT PATH C");
            return Err(anyhow::anyhow!(
                "CRITICAL: Labeler mode exited unexpectedly"
            ));
        }
        "all" => {
            eprintln!("[EXIT-TRACE] Running in 'all' mode");
            // Run all ingesters concurrently
            let firehose_config = config.clone();
            let backfill_config = config.clone();
            let labeler_config = config.clone();

            eprintln!("[EXIT-TRACE] Spawning firehose and backfill tasks");
            // Spawn all tasks and wait for any to error
            let firehose_handle = tokio::spawn(async move { run_firehose(firehose_config).await });
            let backfill_handle = tokio::spawn(async move { run_backfill(backfill_config).await });

            // Only spawn labeler if we have labeler hosts configured
            if !config.labeler_hosts.is_empty() {
                eprintln!("[EXIT-TRACE] Spawning labeler task (hosts configured)");
                let labeler_handle = tokio::spawn(async move { run_labeler(labeler_config).await });

                eprintln!("[EXIT-TRACE] Entering tokio::select! with all three tasks");
                // Use select! to exit if any task exits
                tokio::select! {
                    result = firehose_handle => {
                        eprintln!("[EXIT-TRACE] tokio::select! triggered on firehose_handle - EXIT PATH D");
                        eprintln!("[EXIT-TRACE] Firehose result: {:?}", result);
                        error!("[EXIT-TRACE] tokio::select! triggered on firehose_handle - EXIT PATH D");
                        error!("Firehose ingester exited: {:?}", result);
                        return Err(anyhow::anyhow!("CRITICAL: Firehose ingester exited unexpectedly. This should never happen. Check logs for details."));
                    }
                    result = backfill_handle => {
                        eprintln!("[EXIT-TRACE] tokio::select! triggered on backfill_handle - EXIT PATH E");
                        eprintln!("[EXIT-TRACE] Backfill result: {:?}", result);
                        error!("[EXIT-TRACE] tokio::select! triggered on backfill_handle - EXIT PATH E");
                        error!("Backfill ingester exited: {:?}", result);
                        return Err(anyhow::anyhow!("CRITICAL: Backfill ingester exited unexpectedly. This should never happen. Check logs for details."));
                    }
                    result = labeler_handle => {
                        eprintln!("[EXIT-TRACE] tokio::select! triggered on labeler_handle - EXIT PATH F");
                        eprintln!("[EXIT-TRACE] Labeler result: {:?}", result);
                        error!("[EXIT-TRACE] tokio::select! triggered on labeler_handle - EXIT PATH F");
                        error!("Labeler ingester exited: {:?}", result);
                        return Err(anyhow::anyhow!("CRITICAL: Labeler ingester exited unexpectedly. This should never happen. Check logs for details."));
                    }
                }
            } else {
                // No labeler, just wait for firehose or backfill
                eprintln!("[EXIT-TRACE] No labeler hosts configured");
                info!("No labeler hosts configured, skipping labeler ingester");
                eprintln!("[EXIT-TRACE] Entering tokio::select! with firehose and backfill only");
                tokio::select! {
                    result = firehose_handle => {
                        eprintln!("[EXIT-TRACE] tokio::select! triggered on firehose_handle (no labeler) - EXIT PATH G");
                        eprintln!("[EXIT-TRACE] Firehose result: {:?}", result);
                        error!("[EXIT-TRACE] tokio::select! triggered on firehose_handle (no labeler) - EXIT PATH G");
                        error!("Firehose ingester exited: {:?}", result);
                        return Err(anyhow::anyhow!("CRITICAL: Firehose ingester exited unexpectedly. This should never happen. Check logs for details."));
                    }
                    result = backfill_handle => {
                        eprintln!("[EXIT-TRACE] tokio::select! triggered on backfill_handle (no labeler) - EXIT PATH H");
                        eprintln!("[EXIT-TRACE] Backfill result: {:?}", result);
                        error!("[EXIT-TRACE] tokio::select! triggered on backfill_handle (no labeler) - EXIT PATH H");
                        error!("Backfill ingester exited: {:?}", result);
                        return Err(anyhow::anyhow!("CRITICAL: Backfill ingester exited unexpectedly. This should never happen. Check logs for details."));
                    }
                }
            }
        }
        _ => {
            eprintln!("[EXIT-TRACE] Unknown INGESTER_MODE: {} - EXIT PATH I", mode);
            error!("Unknown INGESTER_MODE: {}", mode);
            std::process::exit(1);
        }
    }
}

async fn run_firehose(config: IngesterConfig) -> Result<()> {
    info!(
        "Starting firehose ingesters for {} hosts",
        config.relay_hosts.len()
    );

    let mut tasks = Vec::new();

    for hostname in &config.relay_hosts {
        let ingester_clone = FirehoseIngester::new(config.clone())?;
        let hostname_clone = hostname.clone();

        let task = tokio::spawn(async move {
            if let Err(e) = ingester_clone.run(hostname_clone.clone()).await {
                error!(
                    "CRITICAL: Firehose ingester for {} exited with error: {:?}\nThis task should never exit. Investigate immediately.",
                    hostname_clone, e
                );
            } else {
                error!(
                    "CRITICAL: Firehose ingester for {} exited successfully but should never exit!\nThis indicates a logic error in the code.",
                    hostname_clone
                );
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks (they should never exit)
    // If any task exits, log it and wait for others
    for (i, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(()) => {
                error!(
                    "CRITICAL: Firehose task {} completed unexpectedly. Tasks should run forever.",
                    i
                );
            }
            Err(e) => {
                error!(
                    "CRITICAL: Firehose task {} panicked or was cancelled: {:?}\nThis indicates a serious error.",
                    i, e
                );
            }
        }
    }

    // If we get here, ALL tasks have exited, which is a critical failure
    Err(anyhow::anyhow!(
        "CRITICAL: All firehose tasks exited. This should never happen."
    ))
}

async fn run_backfill(config: IngesterConfig) -> Result<()> {
    info!(
        "Starting backfill ingesters for {} hosts",
        config.relay_hosts.len()
    );

    let mut tasks = Vec::new();

    for hostname in &config.relay_hosts {
        let ingester_clone = BackfillIngester::new(config.clone())?;
        let hostname_clone = hostname.clone();

        let task = tokio::spawn(async move {
            if let Err(e) = ingester_clone.run(hostname_clone.clone()).await {
                error!(
                    "CRITICAL: Backfill ingester for {} exited with error: {:?}\nThis task should never exit. Investigate immediately.",
                    hostname_clone, e
                );
            } else {
                error!(
                    "CRITICAL: Backfill ingester for {} exited successfully but should never exit!\nThis indicates a logic error in the code.",
                    hostname_clone
                );
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks (they should never exit)
    // If any task exits, log it and wait for others
    for (i, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(()) => {
                error!(
                    "CRITICAL: Backfill task {} completed unexpectedly. Tasks should run forever.",
                    i
                );
            }
            Err(e) => {
                error!(
                    "CRITICAL: Backfill task {} panicked or was cancelled: {:?}\nThis indicates a serious error.",
                    i, e
                );
            }
        }
    }

    // If we get here, ALL tasks have exited, which is a critical failure
    Err(anyhow::anyhow!(
        "CRITICAL: All backfill tasks exited. This should never happen."
    ))
}

async fn run_labeler(config: IngesterConfig) -> Result<()> {
    if config.labeler_hosts.is_empty() {
        info!("No labeler hosts configured, sleeping forever to keep task alive");
        // Sleep forever to keep this task alive
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }

    info!(
        "Starting labeler ingesters for {} hosts",
        config.labeler_hosts.len()
    );

    let mut tasks = Vec::new();

    for hostname in &config.labeler_hosts {
        let ingester_clone = LabelerIngester::new(config.clone())?;
        let hostname_clone = hostname.clone();

        let task = tokio::spawn(async move {
            if let Err(e) = ingester_clone.run(hostname_clone.clone()).await {
                error!(
                    "CRITICAL: Labeler ingester for {} exited with error: {:?}\nThis task should never exit. Investigate immediately.",
                    hostname_clone, e
                );
            } else {
                error!(
                    "CRITICAL: Labeler ingester for {} exited successfully but should never exit!\nThis indicates a logic error in the code.",
                    hostname_clone
                );
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks (they should never exit)
    // If any task exits, log it and wait for others
    for (i, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(()) => {
                error!(
                    "CRITICAL: Labeler task {} completed unexpectedly. Tasks should run forever.",
                    i
                );
            }
            Err(e) => {
                error!(
                    "CRITICAL: Labeler task {} panicked or was cancelled: {:?}\nThis indicates a serious error.",
                    i, e
                );
            }
        }
    }

    // If we get here, ALL tasks have exited, which is a critical failure
    Err(anyhow::anyhow!(
        "CRITICAL: All labeler tasks exited. This should never happen."
    ))
}

fn load_config() -> IngesterConfig {
    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

    let relay_hosts = env::var("INGESTER_RELAY_HOSTS")
        .unwrap_or_else(|_| "bsky.network".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let labeler_hosts = env::var("INGESTER_LABELER_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let high_water_mark = env::var("INGESTER_HIGH_WATER_MARK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    let batch_size = env::var("INGESTER_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    let batch_timeout_ms = env::var("INGESTER_BATCH_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);

    IngesterConfig {
        redis_url,
        relay_hosts,
        labeler_hosts,
        high_water_mark,
        batch_size,
        batch_timeout_ms,
    }
}
