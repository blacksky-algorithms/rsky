//! Blacksky Video Service
//!
//! Handles video uploads, transcoding via Bunny Stream, and playback URL proxying.
//! Implements the app.bsky.video.* lexicon endpoints.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};
use deadpool_postgres::{Config as PgConfig, Runtime};
use tokio_postgres::NoTls;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use rustls::crypto::aws_lc_rs::default_provider;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod auth;
mod bunny;
mod config;
mod db;
mod error;
mod xrpc;

pub use config::AppConfig;
pub use error::{Error, Result};

/// Shared application state
pub struct AppState {
    pub config: AppConfig,
    pub db_pool: deadpool_postgres::Pool,
    pub bunny_client: bunny::BunnyClient,
    pub http_client: reqwest::Client,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Initialize TLS crypto provider
    default_provider().install_default().unwrap();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,rsky_video=debug")),
        )
        .init();

    // Load configuration
    let config = AppConfig::from_env()?;
    info!(
        "Starting Blacksky Video Service on {}:{}",
        config.host, config.port
    );

    // Initialize database pool
    let mut pg_config = PgConfig::new();
    pg_config.url = Some(config.database_url.clone());
    let db_pool = pg_config.create_pool(Some(Runtime::Tokio1), NoTls)?;

    // Run migrations
    db::run_migrations(&db_pool).await?;

    // Initialize Bunny client
    let bunny_client = bunny::BunnyClient::new(
        config.bunny_library_id.clone(),
        config.bunny_api_key.clone(),
        config.bunny_pull_zone.clone(),
    );

    // Initialize HTTP client for PDS uploads
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    // Create shared state
    let state = Arc::new(AppState {
        config: config.clone(),
        db_pool,
        bunny_client,
        http_client,
    });

    // Build router
    let app = Router::new()
        // XRPC endpoints
        .route(
            "/xrpc/app.bsky.video.getUploadLimits",
            get(xrpc::get_upload_limits),
        )
        .route("/xrpc/app.bsky.video.uploadVideo", post(xrpc::upload_video))
        .route(
            "/xrpc/app.bsky.video.getJobStatus",
            get(xrpc::get_job_status),
        )
        // Webhook endpoint for Bunny callbacks
        .route("/webhook/bunny", post(xrpc::bunny_webhook))
        // Video proxy endpoints
        .route("/stream/:did/:cid/playlist.m3u8", get(xrpc::proxy_playlist))
        .route(
            "/stream/:did/:cid/thumbnail.jpg",
            get(xrpc::proxy_thumbnail),
        )
        // Health check
        .route("/health", get(health_check))
        .route("/_health", get(health_check))
        // Add middleware
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}
