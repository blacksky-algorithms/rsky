//! OAuth Client ID type.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// A validated OAuth client ID.
///
/// This is a newtype wrapper around String that ensures the client ID is not empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientId(String);

impl OAuthClientId {
    /// Create a new OAuthClientId.
    ///
    /// # Errors
    /// Returns an error if the client ID string is empty.
    pub fn new(client_id: impl Into<String>) -> Result<Self, OAuthClientIdError> {
        let client_id = client_id.into();
        if client_id.is_empty() {
            return Err(OAuthClientIdError::Empty);
        }
        Ok(Self(client_id))
    }

    /// Get the underlying client ID string.
    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }

    /// Get the underlying client ID string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl AsRef<str> for OAuthClientId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OAuthClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for OAuthClientId {
    type Err = OAuthClientIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur when creating an OAuthClientId.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Serialize, Deserialize)]
pub enum OAuthClientIdError {
    #[error("Client ID cannot be empty")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid_id() {
        let id = OAuthClientId::new("client123").unwrap();
        assert_eq!(id.as_ref(), "client123");
    }

    #[test]
    fn test_new_empty_id() {
        assert!(matches!(
            OAuthClientId::new(""),
            Err(OAuthClientIdError::Empty)
        ));
    }

    #[test]
    fn test_display() {
        let id = OAuthClientId::new("client123").unwrap();
        assert_eq!(id.to_string(), "client123");
    }

    #[test]
    fn test_into_inner() {
        let id = OAuthClientId::new("client123").unwrap();
        assert_eq!(id.into_inner(), "client123");
    }

    #[test]
    fn test_from_str() {
        let id: OAuthClientId = "client123".parse().unwrap();
        assert_eq!(id.as_ref(), "client123");

        let result: Result<OAuthClientId, _> = "".parse();
        assert!(result.is_err());
    }
}
