use anyhow::Result;
use rsky_ingester::{
    backfill::BackfillIngester, firehose::FirehoseIngester, labeler::LabelerIngester,
    IngesterConfig,
};
use std::env;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

    info!("Starting rsky-ingester");

    // Load configuration from environment
    let config = load_config();

    info!("Configuration: {:?}", config);

    // Determine which ingesters to run
    let mode = env::var("INGESTER_MODE").unwrap_or_else(|_| "all".to_string());

    match mode.as_str() {
        "firehose" => {
            run_firehose(config).await?;
        }
        "backfill" => {
            run_backfill(config).await?;
        }
        "labeler" => {
            run_labeler(config).await?;
        }
        "all" => {
            // Run all ingesters concurrently
            let firehose_config = config.clone();
            let backfill_config = config.clone();
            let labeler_config = config.clone();

            tokio::select! {
                result = tokio::spawn(async move { run_firehose(firehose_config).await }) => {
                    error!("Firehose ingester exited: {:?}", result);
                }
                result = tokio::spawn(async move { run_backfill(backfill_config).await }) => {
                    error!("Backfill ingester exited: {:?}", result);
                }
                result = tokio::spawn(async move { run_labeler(labeler_config).await }) => {
                    error!("Labeler ingester exited: {:?}", result);
                }
            }
        }
        _ => {
            error!("Unknown INGESTER_MODE: {}", mode);
            std::process::exit(1);
        }
    }

    Ok(())
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
                error!("Firehose ingester error for {}: {:?}", hostname_clone, e);
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks
    for task in tasks {
        let _ = task.await;
    }

    Ok(())
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
                error!("Backfill ingester error for {}: {:?}", hostname_clone, e);
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks
    for task in tasks {
        let _ = task.await;
    }

    Ok(())
}

async fn run_labeler(config: IngesterConfig) -> Result<()> {
    if config.labeler_hosts.is_empty() {
        info!("No labeler hosts configured, skipping labeler ingester");
        return Ok(());
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
                error!("Labeler ingester error for {}: {:?}", hostname_clone, e);
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks
    for task in tasks {
        let _ = task.await;
    }

    Ok(())
}

fn load_config() -> IngesterConfig {
    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

    let relay_hosts = env::var("RELAY_HOSTS")
        .unwrap_or_else(|_| "bsky.network".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let labeler_hosts = env::var("LABELER_HOSTS")
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
