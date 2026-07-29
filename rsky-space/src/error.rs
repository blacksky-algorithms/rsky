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
    #[error("expected 2 car roots, got {0}")]
    RootCountMismatch(usize),
    #[error("index does not match trusted commit hash")]
    IndexHashMismatch,
    #[error("block bytes do not match cid: {0}")]
    BlockCidMismatch(String),
    #[error("block out of index order: {0}")]
    BlockOrderViolation(String),
    #[error("missing block: {0}")]
    MissingBlock(String),
    #[error("car error: {0}")]
    Car(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("invalid jwk: {0}")]
    InvalidJwk(String),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SpaceError>;
