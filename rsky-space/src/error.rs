use thiserror::Error;

/// Errors surfaced by the permissioned-data (spaces) primitives.
#[derive(Debug, Error)]
pub enum SpaceError {
    #[error("commit signature verification failed")]
    BadSignature,
    #[error("commit MAC verification failed")]
    BadMac,
    #[error("HKDF expansion failed")]
    Hkdf,
    #[error("invalid space uri: {0}")]
    InvalidSpaceUri(String),
    #[error("invalid record uri: {0}")]
    InvalidRecordUri(String),
    #[error("malformed jwt: {0}")]
    MalformedJwt(String),
    #[error("jwt claim invalid: {0}")]
    InvalidClaim(String),
    #[error("credential expired")]
    Expired,
    #[error("not authorized for space")]
    NotAuthorized,
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SpaceError>;
