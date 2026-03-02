//! Error types for the video service

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Upload limit exceeded: {0}")]
    UploadLimitExceeded(String),

    #[error("Video too large: {0}")]
    VideoTooLarge(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(#[from] tokio_postgres::Error),

    #[error("Pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),

    #[error("Bunny API error: {0}")]
    BunnyApi(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            Error::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            Error::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            Error::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Error::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Error::RateLimited(msg) => (StatusCode::TOO_MANY_REQUESTS, msg.clone()),
            Error::UploadLimitExceeded(msg) => (StatusCode::TOO_MANY_REQUESTS, msg.clone()),
            Error::VideoTooLarge(msg) => (StatusCode::PAYLOAD_TOO_LARGE, msg.clone()),
            Error::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            Error::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                )
            }
            Error::Pool(e) => {
                tracing::error!("Pool error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database pool error".to_string(),
                )
            }
            Error::BunnyApi(msg) => {
                tracing::error!("Bunny API error: {}", msg);
                (
                    StatusCode::BAD_GATEWAY,
                    format!("Video service error: {}", msg),
                )
            }
            Error::Http(e) => {
                tracing::error!("HTTP error: {}", e);
                (StatusCode::BAD_GATEWAY, "HTTP request failed".to_string())
            }
            Error::Json(e) => {
                tracing::error!("JSON error: {}", e);
                (StatusCode::BAD_REQUEST, "Invalid JSON".to_string())
            }
        };

        let body = Json(json!({
            "error": error_message,
            "message": error_message,
        }));

        (status, body).into_response()
    }
}
