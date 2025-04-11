use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use url::Url;

/// A validated OAuth request URI.
///
/// This type ensures that the URI is a valid URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthRequestUri(String);

impl OAuthRequestUri {
    /// Create a new OAuthRequestUri.
    ///
    /// # Errors
    /// Returns an error if the URI string is empty or not a valid URL.
    pub fn new(uri: impl Into<String>) -> Result<Self, OAuthRequestUriError> {
        let uri = uri.into();
        if uri.is_empty() {
            return Err(OAuthRequestUriError::Empty);
        }

        // Validate that it's a proper URL
        Url::parse(&uri).map_err(|_| OAuthRequestUriError::InvalidUrl)?;

        Ok(Self(uri))
    }

    /// Get the underlying URI string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for OAuthRequestUri {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OAuthRequestUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for OAuthRequestUri {
    type Err = OAuthRequestUriError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur when creating an OAuthRequestUri.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OAuthRequestUriError {
    #[error("Request URI cannot be empty")]
    Empty,
    #[error("Invalid URL format")]
    InvalidUrl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid_uri() {
        let uri = OAuthRequestUri::new("https://example.com/oauth/request").unwrap();
        assert_eq!(uri.as_ref(), "https://example.com/oauth/request");
    }

    #[test]
    fn test_new_empty_uri() {
        assert!(matches!(
            OAuthRequestUri::new(""),
            Err(OAuthRequestUriError::Empty)
        ));
    }

    #[test]
    fn test_new_invalid_uri() {
        assert!(matches!(
            OAuthRequestUri::new("not a url"),
            Err(OAuthRequestUriError::InvalidUrl)
        ));
    }

    #[test]
    fn test_display() {
        let uri = OAuthRequestUri::new("https://example.com/oauth/request").unwrap();
        assert_eq!(uri.to_string(), "https://example.com/oauth/request");
    }

    #[test]
    fn test_into_inner() {
        let uri = OAuthRequestUri::new("https://example.com/oauth/request").unwrap();
        assert_eq!(uri.into_inner(), "https://example.com/oauth/request");
    }

    #[test]
    fn test_as_ref() {
        let uri = OAuthRequestUri::new("https://example.com/oauth/request").unwrap();
        assert_eq!(uri.as_ref(), "https://example.com/oauth/request");
    }

    #[test]
    fn test_from_str() {
        let uri: OAuthRequestUri = "https://example.com/oauth/request".parse().unwrap();
        assert_eq!(uri.as_ref(), "https://example.com/oauth/request");

        assert!("".parse::<OAuthRequestUri>().is_err());
        assert!("not a url".parse::<OAuthRequestUri>().is_err());
    }
}
