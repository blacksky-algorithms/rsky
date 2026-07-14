use thiserror::Error;

#[derive(Debug, Error)]
pub enum HostError {
    #[error("invalid signing key: {0}")]
    Key(String),
    #[error("delegation token rejected: {0}")]
    Delegation(String),
    #[error("client attestation required")]
    AttestationRequired,
    #[error("client attestation rejected: {0}")]
    Attestation(String),
    #[error("user not authorized for space")]
    NotAuthorized,
    #[error("client not authorized for space")]
    ClientNotAuthorized,
    #[error("membership lookup failed: {0}")]
    Membership(String),
    #[error("managing app check failed: {0}")]
    ManagingApp(String),
    #[error("identity resolution failed: {0}")]
    Resolution(String),
    #[error("store error: {0}")]
    Store(String),
    #[error(transparent)]
    Space(#[from] rsky_space::SpaceError),
}

pub type Result<T> = std::result::Result<T, HostError>;
