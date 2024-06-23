use std::fmt::Display;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum InternalErrorCode {
    #[serde(rename = "no_internal_error")]
    NoInternalError,
    #[serde(rename = "internal_error")]
    InternalError,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "deadline_exceeded")]
    DeadlineExceeded,
    #[serde(rename = "already_exists")]
    AlreadyExists,
    #[serde(rename = "resource_exhausted")]
    ResourceExhausted,
    #[serde(rename = "failed_precondition")]
    FailedPrecondition,
    #[serde(rename = "aborted")]
    Aborted,
    #[serde(rename = "out_of_range")]
    OutOfRange,
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "data_loss")]
    DataLoss,
}

impl Display for InternalErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoInternalError => "no_internal_error",
            Self::InternalError => "internal_error",
            Self::Cancelled => "cancelled",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::AlreadyExists => "already_exists",
            Self::ResourceExhausted => "resource_exhausted",
            Self::FailedPrecondition => "failed_precondition",
            Self::Aborted => "aborted",
            Self::OutOfRange => "out_of_range",
            Self::Unavailable => "unavailable",
            Self::DataLoss => "data_loss",
        })
    }
}

impl Default for InternalErrorCode {
    fn default() -> InternalErrorCode {
        Self::NoInternalError
    }
}
