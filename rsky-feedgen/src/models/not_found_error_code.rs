#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum NotFoundErrorCode {
    #[serde(rename = "not_found_error")]
    NotFoundError,
    #[serde(rename = "undefined_endpoint")]
    UndefinedEndpoint,
    #[serde(rename = "unimplemented")]
    Unimplemented,
}

impl ToString for NotFoundErrorCode {
    fn to_string(&self) -> String {
        match self {
            Self::NotFoundError => String::from("not_found_error"),
            Self::UndefinedEndpoint => String::from("undefined_endpoint"),
            Self::Unimplemented => String::from("unimplemented"),
        }
    }
}

impl Default for NotFoundErrorCode {
    fn default() -> NotFoundErrorCode {
        Self::NotFoundError
    }
}
