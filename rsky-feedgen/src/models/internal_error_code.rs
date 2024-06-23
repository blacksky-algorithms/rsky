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

impl ToString for InternalErrorCode {
    fn to_string(&self) -> String {
        match self {
            Self::NoInternalError => String::from("no_internal_error"),
            Self::InternalError => String::from("internal_error"),
            Self::Cancelled => String::from("cancelled"),
            Self::DeadlineExceeded => String::from("deadline_exceeded"),
            Self::AlreadyExists => String::from("already_exists"),
            Self::ResourceExhausted => String::from("resource_exhausted"),
            Self::FailedPrecondition => String::from("failed_precondition"),
            Self::Aborted => String::from("aborted"),
            Self::OutOfRange => String::from("out_of_range"),
            Self::Unavailable => String::from("unavailable"),
            Self::DataLoss => String::from("data_loss"),
        }
    }
}

impl Default for InternalErrorCode {
    fn default() -> InternalErrorCode {
        Self::NoInternalError
    }
}
