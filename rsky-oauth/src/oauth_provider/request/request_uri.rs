use crate::oauth_provider::request::request_id::RequestId;
use serde::de::Visitor;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

pub const REQUEST_URI_PREFIX: &str = "urn:ietf:params:oauth:request_uri:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestUri(String);

impl RequestUri {
    /// Create a new RequestUri.
    ///
    /// # Errors
    /// Returns an error if the client ID string is empty.
    pub fn new(uri: impl Into<String>) -> Result<Self, RequestUriError> {
        let uri = uri.into();
        if uri.is_empty() {
            return Err(RequestUriError::Empty);
        }
        if !uri.starts_with(REQUEST_URI_PREFIX) {
            return Err(RequestUriError::InvalidFormat);
        }
        Ok(Self(uri))
    }

    pub fn encode(request_id: RequestId) -> RequestUri {
        let val = REQUEST_URI_PREFIX.to_string() + request_id.into_inner().as_str();
        RequestUri::new(val).unwrap()
    }

    pub fn decode(request_uri: &RequestUri) -> RequestId {
        let val = &request_uri.clone().into_inner()[REQUEST_URI_PREFIX.len()..];
        RequestId::new(val).unwrap()
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

impl Serialize for RequestUri {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

// Custom visitor for deserialization
struct RequestUriVisitor;

impl Visitor<'_> for RequestUriVisitor {
    type Value = RequestUri;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string representing a RequestUri")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match RequestUri::new(value) {
            Ok(uri) => Ok(uri),
            Err(error) => Err(E::custom(format!("{:?}", error))),
        }
    }
}

// Implement Deserialize using the visitor
impl<'de> Deserialize<'de> for RequestUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(RequestUriVisitor)
    }
}

/// Errors that can occur when creating an RequestUri.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestUriError {
    Empty,
    InvalidFormat,
}
