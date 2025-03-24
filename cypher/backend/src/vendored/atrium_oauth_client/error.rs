use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ClientMetadata(#[from] crate::vendored::atrium_oauth_client::atproto::Error),
    #[error(transparent)]
    Keyset(#[from] crate::vendored::atrium_oauth_client::keyset::Error),
    #[error(transparent)]
    Identity(#[from] atrium_identity::Error),
    #[error(transparent)]
    ServerAgent(#[from] crate::vendored::atrium_oauth_client::server_agent::Error),
    #[error("authorize error: {0}")]
    Authorize(String),
    #[error("callback error: {0}")]
    Callback(String),
    #[error("state store error: {0:?}")]
    StateStore(Box<dyn std::error::Error + Send + Sync + 'static>),
}

pub type Result<T> = core::result::Result<T, Error>;
