use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("repo host request failed: {0}")]
    RepoHost(String),
    #[error("index error: {0}")]
    Index(String),
    #[error("key resolution failed: {0}")]
    KeyResolution(String),
    /// Our independently-derived set hash disagrees with the signed commit:
    /// the local copy has diverged and must fall back to full-state recovery.
    #[error("repo diverged from signed commit for {0}")]
    Diverged(String),
    #[error("not yet implemented: {0}")]
    NotImplemented(&'static str),
    #[error(transparent)]
    Space(#[from] rsky_space::SpaceError),
}

pub type Result<T> = std::result::Result<T, DaemonError>;
