#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum NotFoundErrorCode {
    #[serde(rename = "no_not_found_error")]
    NoNotFoundError,
    #[serde(rename = "undefined_endpoint")]
    UndefinedEndpoint,
    #[serde(rename = "store_id_not_found")]
    StoreIdNotFound,
    #[serde(rename = "unimplemented")]
    Unimplemented,
}

impl ToString for NotFoundErrorCode {
    fn to_string(&self) -> String {
        match self {
            Self::NoNotFoundError => String::from("no_not_found_error"),
            Self::UndefinedEndpoint => String::from("undefined_endpoint"),
            Self::StoreIdNotFound => String::from("store_id_not_found"),
            Self::Unimplemented => String::from("unimplemented"),
        }
    }
}

impl Default for NotFoundErrorCode {
    fn default() -> NotFoundErrorCode {
        Self::NoNotFoundError
    }
}
