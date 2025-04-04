use crate::oauth_provider::request::request_id::RequestId;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

pub const REQUEST_URI_PREFIX: &str = "urn:ietf:params:oauth:request_uri:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestUri(String);

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
        self.0
    }
}

impl Display for RequestUri {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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

pub fn decode_request_uri(request_uri: &RequestUri) -> RequestId {
    let val = request_uri.clone().into_inner();
    RequestId::new(val).unwrap()
}

pub fn encode_request_uri(request_id: RequestId) -> RequestUri {
    unimplemented!()
}

pub fn decode_request_id(request_id: RequestId) -> RequestUri {
    unimplemented!()
}
