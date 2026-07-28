//! Configuration for the video service

use color_eyre::Result;
use std::env;

/// Application configuration loaded from environment variables
#[derive(Clone)]
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
    /// Bunny Pull Zone subdomain, without the `.b-cdn.net` suffix.
    pub bunny_pull_zone: String,
    /// Bunny URL Token Authentication key (Stream > Library > Security).
    /// Distinct from `bunny_api_key`. Used to sign CDN playback URLs.
    /// `None` when CDN_TOKEN_AUTH=false: playback URLs are left unsigned, for
    /// cutover phases where the pull zone does not enforce token auth yet.
    pub bunny_token_key: Option<String>,
    /// Cache-Control max-age of the playlist 307 redirect, in seconds.
    /// The CDN token TTL is derived from the longest of these max-ages, so a
    /// cached redirect never carries an expired token. Override (together with
    /// the thumbnail one) to drain caches during a token-auth cutover.
    pub playlist_redirect_max_age_secs: i64,
    /// Cache-Control max-age of the thumbnail 307 redirect, in seconds.
    pub thumbnail_redirect_max_age_secs: i64,

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

    /// Wall-clock ceiling for a single ffmpeg invocation, in seconds
    /// (default: 120). A transcode that outlives it is killed.
    pub transcode_timeout_secs: u64,
    /// How many ffmpeg processes may run at once (default: half the available
    /// cores, at least one). Bounds total transcode CPU, memory and temp disk.
    pub transcode_max_concurrent: usize,
    /// How long an upload waits for a transcode slot before being turned away
    /// with 429, in seconds (default: 30).
    pub transcode_queue_timeout_secs: u64,
    /// Ceiling on ffmpeg output size in bytes (default: 2x `max_video_size`).
    /// Guards against expansion bombs, where a small input decodes into an
    /// enormous output.
    pub transcode_max_output_bytes: u64,
    /// Threads a single ffmpeg process may use (default: 2). Keeps one
    /// conversion from saturating every core.
    pub transcode_threads: u32,
}

impl AppConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let cdn_token_auth = env::var("CDN_TOKEN_AUTH")
            .map(|v| v.parse().expect("CDN_TOKEN_AUTH must be 'true' or 'false'"))
            .unwrap_or(true);
        let bunny_token_key = if cdn_token_auth {
            Some(env::var("BUNNY_TOKEN_KEY").expect("BUNNY_TOKEN_KEY must be set"))
        } else {
            None
        };

        let max_video_size: u64 = env::var("MAX_VIDEO_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100_000_000); // 100MB

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
            bunny_token_key,
            // Cutover knobs: fail loudly on unparseable values instead of
            // silently falling back to the long defaults.
            playlist_redirect_max_age_secs: env::var("PLAYLIST_REDIRECT_MAX_AGE_SECS")
                .map(|s| {
                    s.parse()
                        .expect("PLAYLIST_REDIRECT_MAX_AGE_SECS must be an integer")
                })
                .unwrap_or(3600), // 1 hour
            thumbnail_redirect_max_age_secs: env::var("THUMBNAIL_REDIRECT_MAX_AGE_SECS")
                .map(|s| {
                    s.parse()
                        .expect("THUMBNAIL_REDIRECT_MAX_AGE_SECS must be an integer")
                })
                .unwrap_or(86400), // 24 hours

            service_did: env::var("VIDEO_SERVICE_DID")
                .unwrap_or_else(|_| "did:web:video.blacksky.community".to_string()),
            public_url: env::var("VIDEO_PUBLIC_URL")
                .unwrap_or_else(|_| "https://video.blacksky.community".to_string()),
            signing_key_path: env::var("SIGNING_KEY_PATH").ok(),

            max_video_size,
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

            transcode_timeout_secs: env::var("TRANSCODE_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(120),
            transcode_max_concurrent: env::var("TRANSCODE_MAX_CONCURRENT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_transcode_concurrency)
                .max(1),
            transcode_queue_timeout_secs: env::var("TRANSCODE_QUEUE_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            transcode_max_output_bytes: env::var("TRANSCODE_MAX_OUTPUT_BYTES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(max_video_size.saturating_mul(2)),
            transcode_threads: env::var("TRANSCODE_THREADS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2)
                .max(1),
        })
    }
}

/// Half the available cores, at least one -- leaves headroom for the request
/// path while a transcode runs.
fn default_transcode_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get() / 2)
        .unwrap_or(1)
        .max(1)
}
