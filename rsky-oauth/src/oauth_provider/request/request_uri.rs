use crate::oauth_provider::request::request_id::RequestId;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

pub const REQUEST_URI_PREFIX: &str = "urn:ietf:params:oauth:request_uri:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestUri {
    uri: String,
    request_id: RequestId,
}

impl RequestUri {
    /// Create a new RequestUri.
    ///
    /// # Errors
    /// Returns an error if the client ID string is empty.
    pub fn new(uri: impl Into<String>) -> Result<Self, RequestUriError> {
        unimplemented!()
    }

    /// Get the underlying client ID string.
    pub fn into_inner(self) -> String {
        unimplemented!()
        // self.0
    }

    pub fn request_id(&self) -> RequestId {
        unimplemented!()
        // self.0[..REQUEST_URI_PREFIX.len()]
    }
}

impl Display for RequestUri {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        unimplemented!()
        // self.0.fmt(f)
    }
}

impl FromStr for RequestUri {
    type Err = RequestUriError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur when creating an RequestUri.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestUriError {
    Empty,
}
