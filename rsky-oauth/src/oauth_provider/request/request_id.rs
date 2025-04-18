use crate::oauth_provider::constants::{REQUEST_ID_BYTES_LENGTH, REQUEST_ID_PREFIX};
use rand::distr::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

const REQUEST_ID_LENGTH: usize = REQUEST_ID_PREFIX.len() + REQUEST_ID_BYTES_LENGTH; // hex encoding

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub struct RequestId(String);

impl RequestId {
    /// Create a new RequestId.
    ///
    /// # Errors
    /// Returns an error if the client ID string is empty.
    pub fn new(uri: impl Into<String>) -> Result<Self, RequestIdError> {
        let uri = uri.into();
        if uri.is_empty() {
            return Err(RequestIdError::Empty);
        }
        if uri.len() != REQUEST_ID_LENGTH {
            return Err(RequestIdError::InvalidLength);
        }
        if !uri.starts_with(REQUEST_ID_PREFIX) {
            return Err(RequestIdError::InvalidFormat);
        }
        Ok(Self(uri))
    }

    /// Get the underlying client ID string.
    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn generate() -> RequestId {
        let token: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(REQUEST_ID_BYTES_LENGTH)
            .map(char::from)
            .collect();
        let val = REQUEST_ID_PREFIX.to_string() + token.as_str();
        RequestId::new(val).unwrap()
    }
}

impl Display for RequestId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for RequestId {
    type Err = RequestIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur when creating an RequestUri.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestIdError {
    Empty,
    InvalidLength,
    InvalidFormat,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_provider::token::token_id::{TokenId, TokenIdError};

    #[test]
    fn test_request_id() {
        let request_id = RequestId::new("tok-dwadwdaddwadwdad").unwrap();
        assert_eq!(request_id.into_inner(), "tok-dwadwdaddwadwdad");
        let request_id = RequestId::generate();
        let val = request_id.into_inner();
        RequestId::new(val).unwrap();

        let invalid_format = RequestId::new("aaaadwadwdaddwadwdad").unwrap_err();
        assert_eq!(invalid_format, RequestIdError::InvalidFormat);

        let invalid_length = RequestId::new("tok-dwadwda").unwrap_err();
        assert_eq!(invalid_length, RequestIdError::InvalidLength);
    }
}
