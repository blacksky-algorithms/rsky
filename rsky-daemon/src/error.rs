use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("xrpc request failed: {0}")]
    Xrpc(String),
    #[error("index error: {0}")]
    Index(String),
    #[error("key resolution failed: {0}")]
    KeyResolution(String),
    /// Our independently-derived set hash disagrees with the signed commit:
    /// the local copy has diverged and must fall back to full-state recovery.
    #[error("repo diverged from signed commit for {0}")]
    Diverged(String),
    /// The host's oplog no longer covers our `since` revision; fall back to
    /// full-state recovery (`getRepo`).
    #[error("history unavailable: {0}")]
    HistoryUnavailable(String),
    #[error(transparent)]
    Space(#[from] rsky_space::SpaceError),
}

pub type Result<T> = std::result::Result<T, DaemonError>;
