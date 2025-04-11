use crate::oauth_provider::constants::{REQUEST_ID_BYTES_LENGTH, REQUEST_ID_PREFIX};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

const REQUEST_ID_LENGTH: usize = REQUEST_ID_PREFIX.len() + REQUEST_ID_BYTES_LENGTH * 2; // hex encoding

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
        Ok(Self(uri))
    }

    /// Get the underlying client ID string.
    pub fn into_inner(self) -> String {
        self.0
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
}

pub async fn generate_request_id() -> RequestId {
    let val = REQUEST_ID_PREFIX.to_string(); //+ random_hex_id(REQUEST_ID_BYTES_LENGTH);
    RequestId(val)
}
