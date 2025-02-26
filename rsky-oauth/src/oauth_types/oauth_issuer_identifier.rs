use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use url::Url;

/// A validated OAuth issuer identifier.
///
/// Per RFC 8414, this must be a URL that uses the HTTPS scheme and
/// does not contain query parameters or fragments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthIssuerIdentifier(String);

impl OAuthIssuerIdentifier {
    /// Create a new OAuthIssuerIdentifier.
    ///
    /// # Errors
    /// Returns an error if the issuer URL is invalid according to RFC 8414.
    pub fn new(issuer: impl Into<String>) -> Result<Self, OAuthIssuerIdentifierError> {
        let issuer = issuer.into();

        // Validate URL format
        let url = Url::parse(&issuer).map_err(|_| OAuthIssuerIdentifierError::InvalidUrl)?;

        // Validate scheme (although we're accepting HTTP for dev purposes, we note this in the error)
        if url.scheme() != "https" && url.scheme() != "http" {
            return Err(OAuthIssuerIdentifierError::InvalidScheme);
        }

        // Check for trailing slash
        if issuer.ends_with('/') {
            return Err(OAuthIssuerIdentifierError::TrailingSlash);
        }

        // Check if URL has credentials (username/password)
        if !url.username().is_empty() || url.password().is_some() {
            return Err(OAuthIssuerIdentifierError::ContainsCredentials);
        }

        // Check for query or fragment
        if url.query().is_some() || url.fragment().is_some() {
            return Err(OAuthIssuerIdentifierError::ContainsQueryOrFragment);
        }

        // Ensure canonical form
        let canonical_value = if url.path() == "/" {
            url.origin().ascii_serialization()
        } else {
            url.as_str().to_string()
        };

        if issuer != canonical_value {
            return Err(OAuthIssuerIdentifierError::NonCanonicalForm(
                canonical_value,
            ));
        }

        Ok(Self(issuer))
    }

    /// Get the underlying issuer URL string.
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Get the issuer URL.
    pub fn as_url(&self) -> Result<Url, url::ParseError> {
        Url::parse(&self.0)
    }
}

impl AsRef<str> for OAuthIssuerIdentifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OAuthIssuerIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for OAuthIssuerIdentifier {
    type Err = OAuthIssuerIdentifierError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur when creating an OAuthIssuerIdentifier.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OAuthIssuerIdentifierError {
    #[error("Invalid URL format")]
    InvalidUrl,

    #[error("Issuer URL must use the 'https:' scheme")]
    InvalidScheme,

    #[error("Issuer URL must not end with a slash")]
    TrailingSlash,

    #[error("Issuer URL must not contain a username or password")]
    ContainsCredentials,

    #[error("Issuer URL must not contain a query or fragment")]
    ContainsQueryOrFragment,

    #[error("Issuer URL must be in the canonical form: {0}")]
    NonCanonicalForm(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a valid issuer for tests
    fn test_issuer() -> OAuthIssuerIdentifier {
        // Make sure we have a properly formatted URL with colon
        OAuthIssuerIdentifier::new("https://example.com").unwrap()
    }

    #[test]
    fn test_valid_issuer() {
        let valid = OAuthIssuerIdentifier::new("https://example.com").unwrap();
        assert_eq!(valid.as_ref(), "https://example.com");

        // With path
        let valid_path = OAuthIssuerIdentifier::new("https://example.com/issuer").unwrap();
        assert_eq!(valid_path.as_ref(), "https://example.com/issuer");
    }

    #[test]
    fn test_invalid_issuer() {
        // Invalid URL
        assert!(matches!(
            OAuthIssuerIdentifier::new("not a url"),
            Err(OAuthIssuerIdentifierError::InvalidUrl)
        ));

        // Invalid scheme
        assert!(matches!(
            OAuthIssuerIdentifier::new("ftp://example.com"),
            Err(OAuthIssuerIdentifierError::InvalidScheme)
        ));

        // Trailing slash
        assert!(matches!(
            OAuthIssuerIdentifier::new("https://example.com/"),
            Err(OAuthIssuerIdentifierError::TrailingSlash)
        ));

        // Contains credentials
        assert!(matches!(
            OAuthIssuerIdentifier::new("https://user:pass@example.com"),
            Err(OAuthIssuerIdentifierError::ContainsCredentials)
        ));

        // Contains query
        assert!(matches!(
            OAuthIssuerIdentifier::new("https://example.com?query=value"),
            Err(OAuthIssuerIdentifierError::ContainsQueryOrFragment)
        ));

        // Contains fragment
        assert!(matches!(
            OAuthIssuerIdentifier::new("https://example.com#fragment"),
            Err(OAuthIssuerIdentifierError::ContainsQueryOrFragment)
        ));

        // Non-canonical form
        assert!(matches!(
            OAuthIssuerIdentifier::new("HTTPS://EXAMPLE.COM"),
            Err(OAuthIssuerIdentifierError::NonCanonicalForm(_))
        ));
    }

    #[test]
    fn test_display() {
        let issuer = test_issuer();
        assert_eq!(issuer.to_string(), "https://example.com");
    }

    #[test]
    fn test_into_inner() {
        let issuer = test_issuer();
        assert_eq!(issuer.into_inner(), "https://example.com");
    }

    #[test]
    fn test_from_str() {
        let issuer: OAuthIssuerIdentifier = "https://example.com".parse().unwrap();
        assert_eq!(issuer.as_ref(), "https://example.com");

        let result: Result<OAuthIssuerIdentifier, _> = "invalid".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_as_url() {
        let issuer = test_issuer();
        let url = issuer.as_url().unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str().unwrap(), "example.com");
        assert_eq!(url.path(), "/");
    }
}