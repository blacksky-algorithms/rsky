use thiserror::Error;

#[derive(Error, Debug)]
pub enum ErrorKind {
    #[error("Invalid handle")]
    InvalidHandle,
    #[error("Handle not available")]
    HandleNotAvailable,
    #[error("Unsupported domain")]
    UnsupportedDomain,
    #[error("Internal error")]
    InternalError,
}

#[derive(Error, Debug)]
#[error("{kind}: {message}")]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
