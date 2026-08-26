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
    /// A projection destination was unreachable or overloaded. Distinct from
    /// [`DaemonError::Xrpc`] because it must not consume the batch's failure
    /// budget: an outage is not a bad batch.
    #[error("projection destination unavailable: {0}")]
    RetryableProjection(String),
    /// A projection destination refused the batch because its author is not
    /// admitted to the space. Admission is state, not a property of the batch:
    /// it flips when a membership write propagates, so this must not spend the
    /// poison budget. It gets its own slow budget instead.
    #[error("projection destination denied admission: {0}")]
    AdmissionDenied(String),
    #[error(transparent)]
    Space(#[from] rsky_space::SpaceError),
}

impl DaemonError {
    pub fn is_retryable_projection(&self) -> bool {
        matches!(self, Self::RetryableProjection(_))
    }

    pub fn is_admission_denied(&self) -> bool {
        matches!(self, Self::AdmissionDenied(_))
    }
}

pub type Result<T> = std::result::Result<T, DaemonError>;
