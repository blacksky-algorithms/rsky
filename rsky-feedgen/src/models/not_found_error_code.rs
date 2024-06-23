use std::fmt::Display;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum NotFoundErrorCode {
    #[serde(rename = "not_found_error")]
    NotFoundError,
    #[serde(rename = "undefined_endpoint")]
    UndefinedEndpoint,
    #[serde(rename = "unimplemented")]
    Unimplemented,
}

impl Display for NotFoundErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotFoundError => "not_found_error",
            Self::UndefinedEndpoint => "undefined_endpoint",
            Self::Unimplemented => "unimplemented",
        })
    }
}

impl Default for NotFoundErrorCode {
    fn default() -> NotFoundErrorCode {
        Self::NotFoundError
    }
}
