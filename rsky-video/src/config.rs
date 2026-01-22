//! Configuration for the video service

use color_eyre::Result;
use std::env;

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Host to bind to
    pub host: String,
    /// Port to listen on
    pub port: u16,
    /// Database connection URL
    pub database_url: String,

    /// Bunny Stream Library ID
    pub bunny_library_id: String,
    /// Bunny Stream API Key
    pub bunny_api_key: String,
    /// Bunny Pull Zone hostname (e.g., "blacksky-video.b-cdn.net")
    pub bunny_pull_zone: String,

    /// This service's DID (e.g., "did:web:video.blacksky.community")
    pub service_did: String,
    /// Public URL of this service
    pub public_url: String,
    /// Path to the signing key PEM file
    pub signing_key_path: Option<String>,

    /// Maximum video file size in bytes (default: 100MB)
    pub max_video_size: u64,
    /// Maximum video duration in seconds (default: 90)
    pub max_video_duration: u32,
    /// Daily video upload limit per user
    pub daily_video_limit: u32,
    /// Daily byte upload limit per user (default: 10GB)
    pub daily_byte_limit: u64,
}

impl AppConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            host: env::var("VIDEO_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("VIDEO_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3500),
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),

            bunny_library_id: env::var("BUNNY_LIBRARY_ID").expect("BUNNY_LIBRARY_ID must be set"),
            bunny_api_key: env::var("BUNNY_API_KEY").expect("BUNNY_API_KEY must be set"),
            bunny_pull_zone: env::var("BUNNY_PULL_ZONE").expect("BUNNY_PULL_ZONE must be set"),

            service_did: env::var("VIDEO_SERVICE_DID")
                .unwrap_or_else(|_| "did:web:video.blacksky.community".to_string()),
            public_url: env::var("VIDEO_PUBLIC_URL")
                .unwrap_or_else(|_| "https://video.blacksky.community".to_string()),
            signing_key_path: env::var("SIGNING_KEY_PATH").ok(),

            max_video_size: env::var("MAX_VIDEO_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100_000_000), // 100MB
            max_video_duration: env::var("MAX_VIDEO_DURATION")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(90), // 90 seconds
            daily_video_limit: env::var("DAILY_VIDEO_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(25),
            daily_byte_limit: env::var("DAILY_BYTE_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10_737_418_240), // 10GB
        })
    }
}
