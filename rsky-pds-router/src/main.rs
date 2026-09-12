use clap::Parser;
use rsky_pds_router::config::Config;
use rsky_pds_router::journal::Journal;
use rsky_pds_router::lookup::AccountLookup;
use rsky_pds_router::policy::Routing;
use rsky_pds_router::server::{app, metrics_app, Deadlines, Router, Upstreams};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let config = Config::parse();
    let routing = Arc::new(Routing::new(
        config.policy_file.clone(),
        config.allowlist_file.clone(),
    ));
    routing.load()?;
    routing.spawn_reloader(Duration::from_secs(2));
    let journal = Journal::open(&config.journal_path())?;
    let lookup = AccountLookup::open(&config.account_db)?;
    let router = Arc::new(Router::new(
        routing,
        Upstreams {
            ts_main: config.ts_main.clone(),
            ts_sync: config.ts_sync.clone(),
            ts_read: config.ts_read.clone(),
            rsky: config.rsky.clone(),
            oauth: config.oauth.clone(),
        },
        Deadlines {
            read: Duration::from_secs(config.read_timeout_secs),
            sync: Duration::from_secs(config.sync_timeout_secs),
            write: Duration::from_secs(config.write_timeout_secs),
        },
        journal,
        lookup,
    ));
    let metrics_listener =
        tokio::net::TcpListener::bind(("127.0.0.1", config.metrics_port)).await?;
    tokio::spawn(async move {
        if let Err(err) = axum::serve(metrics_listener, metrics_app()).await {
            tracing::error!(%err, "metrics server stopped");
        }
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", config.port)).await?;
    tracing::info!(port = config.port, "router listening");
    axum::serve(listener, app(router))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("router stopped");
    Ok(())
}

/// Resolves on SIGINT or SIGTERM so in-flight requests finish before exit.
async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
    tracing::info!("shutdown requested");
}
