use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_types::{extract_url_path, is_hostname_ip, OAuthClientId};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use url::Url;

/// A discoverable OAuth client ID that is also a valid HTTPS URL.
///
/// Per draft-parecki-oauth-client-id-metadata-document, a discoverable client ID must:
/// - Be a valid HTTPS URL
/// - Not contain credentials (username/password)
/// - Not contain a fragment
/// - Contain a path component
/// - Not end with a trailing slash
/// - Not use an IP address as hostname
/// - Be in canonical form
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientIdDiscoverable(String);

impl OAuthClientIdDiscoverable {
    /// Create a new discoverable client ID.
    pub fn new(client_id: impl Into<String>) -> Result<Self, DiscoverableClientIdError> {
        let client_id = client_id.into();

        // Parse as URL
        let url = Url::parse(&client_id).map_err(|_| DiscoverableClientIdError::InvalidUrl)?;

        // Must be HTTPS
        if url.scheme() != "https" {
            return Err(DiscoverableClientIdError::NotHttps);
        }

        // Check for credentials
        if !url.username().is_empty() || url.password().is_some() {
            return Err(DiscoverableClientIdError::ContainsCredentials);
        }

        // Check for fragment
        if url.fragment().is_some() {
            return Err(DiscoverableClientIdError::ContainsFragment);
        }

        // Must have a path other than "/"
        if url.path() == "/" {
            return Err(DiscoverableClientIdError::NoPath);
        }

        // Must not end with slash
        if url.path().ends_with('/') {
            return Err(DiscoverableClientIdError::TrailingSlash);
        }

        // Must not use IP address as hostname
        if let Some(host) = url.host_str() {
            if is_hostname_ip(host) {
                return Err(DiscoverableClientIdError::IpAddressHost);
            }
        }

        // Check for canonical form
        let extracted_path = extract_url_path(&client_id)
            .map_err(|_| DiscoverableClientIdError::NonCanonicalForm)?;
        if extracted_path != url.path() {
            return Err(DiscoverableClientIdError::NonCanonicalForm);
        }

        Ok(Self(client_id))
    }

    /// Get the client ID as a URL.
    pub fn as_url(&self) -> Url {
        // We can unwrap here because we validated the URL in new()
        Url::parse(&self.0).unwrap()
    }
}

impl AsRef<str> for OAuthClientIdDiscoverable {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OAuthClientIdDiscoverable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for OAuthClientIdDiscoverable {
    type Err = DiscoverableClientIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur with discoverable client IDs.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiscoverableClientIdError {
    #[error("Invalid URL format")]
    InvalidUrl,

    #[error("Client ID must use HTTPS scheme")]
    NotHttps,

    #[error("Client ID must not contain credentials")]
    ContainsCredentials,

    #[error("Client ID must not contain a fragment")]
    ContainsFragment,

    #[error("Client ID must contain a path component")]
    NoPath,

    #[error("Client ID path must not end with a trailing slash")]
    TrailingSlash,

    #[error("Client ID hostname must not be an IP address")]
    IpAddressHost,

    #[error("Client ID must be in canonical form")]
    NonCanonicalForm,
}

impl From<OAuthClientIdDiscoverable> for OAuthClientId {
    fn from(discoverable: OAuthClientIdDiscoverable) -> Self {
        // We can unwrap here because we know it's a valid client ID
        OAuthClientId::new(discoverable.0).unwrap()
    }
}

/// Check if a client ID is a discoverable client ID.
pub fn is_oauth_client_id_discoverable(client_id: &OAuthClientId) -> bool {
    OAuthClientIdDiscoverable::new(client_id.to_string().as_str()).is_ok()
}

/// Assert that a client ID is a discoverable client ID.
pub fn assert_oauth_discoverable_client_id(
    client_id: &str,
) -> Result<(), DiscoverableClientIdError> {
    OAuthClientIdDiscoverable::new(client_id).map(|_| ())
}

/// Parse a client ID into a URL, validating it as a discoverable client ID.
pub fn parse_oauth_discoverable_client_id(
    client_id: &str,
) -> Result<Url, DiscoverableClientIdError> {
    OAuthClientIdDiscoverable::new(client_id).map(|id| id.as_url())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_client_ids() {
        let valid_ids = vec![
            "https://example.com/client.json",
            "https://example.com/clients/123",
            "https://auth.example.com/client-metadata.json",
        ];

        for id in valid_ids {
            assert!(
                OAuthClientIdDiscoverable::new(id).is_ok(),
                "Should accept {}",
                id
            );
        }
    }

    #[test]
    fn test_invalid_client_ids() {
        let test_cases = vec![
            (
                "http://example.com/client",
                DiscoverableClientIdError::NotHttps,
            ),
            (
                "https://user:pass@example.com/client",
                DiscoverableClientIdError::ContainsCredentials,
            ),
            (
                "https://example.com/client#fragment",
                DiscoverableClientIdError::ContainsFragment,
            ),
            ("https://example.com/", DiscoverableClientIdError::NoPath),
            (
                "https://example.com/client/",
                DiscoverableClientIdError::TrailingSlash,
            ),
            (
                "https://127.0.0.1/client",
                DiscoverableClientIdError::IpAddressHost,
            ),
            (
                "https://[::1]/client",
                DiscoverableClientIdError::IpAddressHost,
            ),
        ];

        for (id, expected_error) in test_cases {
            let result = OAuthClientIdDiscoverable::new(id);
            assert!(
                matches!(result, Err(ref e) if e == &expected_error),
                "For {}, expected {:?}, got {:?}",
                id,
                expected_error,
                result
            );
        }
    }

    #[test]
    fn test_canonical_form() {
        let non_canonical = "HTTPS://EXAMPLE.COM/PATH";
        let result = OAuthClientIdDiscoverable::new(non_canonical);
        assert!(matches!(
            result,
            Err(DiscoverableClientIdError::NonCanonicalForm)
        ));

        let canonical = "https://example.com/PATH";
        assert!(OAuthClientIdDiscoverable::new(canonical).is_ok());
    }

    #[test]
    fn test_as_url() {
        let id = OAuthClientIdDiscoverable::new("https://example.com/client").unwrap();
        let url = id.as_url();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str().unwrap(), "example.com");
        assert_eq!(url.path(), "/client");
    }

    #[test]
    fn test_display() {
        let id = OAuthClientIdDiscoverable::new("https://example.com/client").unwrap();
        assert_eq!(id.to_string(), "https://example.com/client");
    }

    #[test]
    fn test_from_str() {
        let id: OAuthClientIdDiscoverable = "https://example.com/client".parse().unwrap();
        assert_eq!(id.to_string(), "https://example.com/client");

        let result: Result<OAuthClientIdDiscoverable, _> = "invalid".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_helper_functions() {
        let valid_id = "https://example.com/client";
        assert!(is_oauth_client_id_discoverable(valid_id));
        assert!(assert_oauth_discoverable_client_id(valid_id).is_ok());
        assert!(parse_oauth_discoverable_client_id(valid_id).is_ok());

        let invalid_id = "invalid";
        assert!(!is_oauth_client_id_discoverable(invalid_id));
        assert!(assert_oauth_discoverable_client_id(invalid_id).is_err());
        assert!(parse_oauth_discoverable_client_id(invalid_id).is_err());
    }
}
