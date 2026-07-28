//! Blacksky Video Service
//!
//! Handles video uploads, transcoding via Bunny Stream, and playback URL proxying.
//! Implements the app.bsky.video.* lexicon endpoints.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use deadpool_postgres::{Config as PgConfig, Runtime};
use rustls::crypto::aws_lc_rs::default_provider;
use tokio_postgres::NoTls;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod auth;
mod bunny;
mod config;
mod db;
mod error;
mod pds;
mod signing;
mod transcode;
mod xrpc;

pub use config::AppConfig;
pub use error::{Error, Result};

/// Shared application state
pub struct AppState {
    pub config: AppConfig,
    pub db_pool: deadpool_postgres::Pool,
    pub bunny_client: bunny::BunnyClient,
    pub pds_client: pds::PdsClient,
    pub http_client: reqwest::Client,
    pub signer: Option<signing::ServiceAuthSigner>,
    pub transcode_limits: transcode::Limits,
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
        config.bunny_token_key.clone(),
        config
            .playlist_redirect_max_age_secs
            .max(config.thumbnail_redirect_max_age_secs),
    );

    // Initialize HTTP client for PDS uploads
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    // Initialize PDS client
    let pds_client = pds::PdsClient::new(http_client.clone());

    // Initialize service auth signer if key is configured
    let signer = match &config.signing_key_path {
        Some(path) => {
            match signing::ServiceAuthSigner::from_pem_file(path, config.service_did.clone()) {
                Ok(s) => {
                    info!("Service auth signing enabled");
                    Some(s)
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to load signing key, PDS uploads will not work: {}",
                        e
                    );
                    None
                }
            }
        }
        None => {
            tracing::warn!(
                "No signing key configured (SIGNING_KEY_PATH), PDS uploads will not work"
            );
            None
        }
    };

    // Resource ceilings for ffmpeg conversions
    let transcode_limits = transcode::Limits::from_config(&config);
    info!(
        "Transcode limits: {} concurrent, {}s deadline, {}s queue wait, {} byte output cap, {} threads each",
        config.transcode_max_concurrent,
        config.transcode_timeout_secs,
        config.transcode_queue_timeout_secs,
        config.transcode_max_output_bytes,
        config.transcode_threads,
    );

    // Create shared state
    let state = Arc::new(AppState {
        config: config.clone(),
        db_pool,
        bunny_client,
        pds_client,
        http_client,
        signer,
        transcode_limits,
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
        // `upload_video` takes the body as `Bytes`, so axum buffers the whole
        // request in memory before the handler's own size check can run. Cap
        // the layer at the same limit so an oversized upload is refused with
        // 413 while streaming, instead of being allocated first.
        .layer(DefaultBodyLimit::max(config.max_video_size as usize))
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
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Resolves on SIGTERM or SIGINT.
///
/// A redeploy sends SIGTERM; without this the process dies immediately and any
/// in-flight upload is lost after its job row was already created, leaving the
/// job stuck. Draining lets running uploads finish first.
async fn shutdown_signal() {
    let interrupt = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => info!("SIGINT received, draining in-flight requests"),
        _ = terminate => info!("SIGTERM received, draining in-flight requests"),
    }
}

async fn health_check() -> &'static str {
    "OK"
}
