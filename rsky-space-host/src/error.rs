use thiserror::Error;

#[derive(Debug, Error)]
pub enum HostError {
    #[error("invalid signing key: {0}")]
    Key(String),
    #[error("delegation token rejected: {0}")]
    Delegation(String),
    #[error("user not authorized for space")]
    NotAuthorized,
    #[error("client not authorized for space")]
    ClientNotAuthorized,
    #[error("membership lookup failed: {0}")]
    Membership(String),
    #[error(transparent)]
    Space(#[from] rsky_space::SpaceError),
}

pub type Result<T> = std::result::Result<T, HostError>;
