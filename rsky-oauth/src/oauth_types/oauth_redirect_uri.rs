use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use url::Url;

use crate::oauth_types::{
    ApplicationType, HttpsUri, LoopbackUri, PrivateUseUri, UriError, ValidUri,
};

/// A validated OAuth redirect URI.
///
/// Can be one of three types:
/// - HTTPS URI (for web clients)
/// - Loopback URI (for native clients during development)
/// - Private-use URI (for native clients)
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum OAuthRedirectUri {
    /// Standard HTTPS redirect URI
    Https(HttpsUri),
    /// Loopback redirect URI for native clients
    Loopback(LoopbackUri),
    /// Private-use URI scheme for native clients
    PrivateUse(PrivateUseUri),
}

impl OAuthRedirectUri {
    /// Creates a new redirect URI of the appropriate type.
    pub fn new(uri: impl AsRef<str>) -> Result<Self, RedirectUriError> {
        let uri_str = uri.as_ref();

        // Try each type in order
        if uri_str.starts_with("https://") {
            Ok(Self::Https(
                HttpsUri::validate(uri_str).map_err(RedirectUriError::InvalidUri)?,
            ))
        } else if uri_str.starts_with("http://") {
            Ok(Self::Loopback(
                LoopbackUri::validate(uri_str).map_err(RedirectUriError::InvalidUri)?,
            ))
        } else {
            // Try as private-use URI
            let result = PrivateUseUri::validate(uri_str).map_err(RedirectUriError::InvalidUri)?;

            // Validate private-use requirements from RFC 8252
            validate_private_use_uri(uri_str)?;

            Ok(Self::PrivateUse(result))
        }
    }

    /// Returns true if this is an HTTPS URI.
    pub fn is_https(&self) -> bool {
        matches!(self, Self::Https(_))
    }

    /// Returns true if this is a loopback URI.
    pub fn is_loopback(&self) -> bool {
        matches!(self, Self::Loopback(_))
    }

    /// Returns true if this is a private-use URI.
    pub fn is_private_use(&self) -> bool {
        matches!(self, Self::PrivateUse(_))
    }

    /// Get the underlying URI string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Https(uri) => uri.as_str(),
            Self::Loopback(uri) => uri.as_str(),
            Self::PrivateUse(uri) => uri.as_str(),
        }
    }

    /// Validate that this redirect URI is allowed for the given application type.
    pub fn validate_for_app_type(&self, app_type: ApplicationType) -> Result<(), RedirectUriError> {
        match (app_type, self) {
            (ApplicationType::Web, Self::Https(_)) => Ok(()),
            (ApplicationType::Web, _) => Err(RedirectUriError::InvalidForWebApp),
            (ApplicationType::Native, Self::PrivateUse(_))
            | (ApplicationType::Native, Self::Loopback(_))
            | (ApplicationType::Native, Self::Https(_)) => Ok(()),
        }
    }
}

impl fmt::Display for OAuthRedirectUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for OAuthRedirectUri {
    type Err = RedirectUriError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for OAuthRedirectUri {
    type Error = RedirectUriError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<OAuthRedirectUri> for String {
    fn from(uri: OAuthRedirectUri) -> Self {
        uri.as_str().to_string()
    }
}

impl AsRef<str> for OAuthRedirectUri {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Errors that can occur when working with redirect URIs.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RedirectUriError {
    #[error("Invalid URI: {0}")]
    InvalidUri(#[from] UriError),

    #[error("Web applications must use HTTPS redirect URIs")]
    InvalidForWebApp,

    #[error("Private-use URI scheme must match client ID hostname")]
    PrivateUseClientMismatch,

    #[error("Private-use URI must have exactly one slash after scheme")]
    InvalidPrivateUsePath,
}

/// Validate a private-use URI according to RFC 8252.
fn validate_private_use_uri(uri_str: &str) -> Result<(), RedirectUriError> {
    // Check for colon, which separates scheme from path
    let colon_idx = uri_str.find(':').ok_or(UriError::InvalidUrl)?;
    let scheme = &uri_str[..colon_idx];

    // Protocol (scheme) must contain a dot per RFC 7595
    if !scheme.contains('.') {
        return Err(RedirectUriError::InvalidUri(
            UriError::InvalidPrivateUseScheme,
        ));
    }

    // Check path part (after the colon)
    let path_part = &uri_str[colon_idx + 1..];

    // According to RFC 8252, there must be exactly one slash after the scheme
    if !path_part.starts_with('/') {
        return Err(RedirectUriError::InvalidUri(UriError::InvalidUrl));
    }

    // Check if there's more than one slash (which would indicate a hostname)
    if path_part.len() > 1 && path_part[1..].contains('/') {
        return Err(RedirectUriError::InvalidPrivateUsePath);
    }

    Ok(())
}

/// Check if a private-use URI matches a client ID host.
pub fn validate_private_use_client_match(
    uri: &str,
    client_id: &str,
) -> Result<bool, RedirectUriError> {
    let uri_url = Url::parse(uri).map_err(|_| UriError::InvalidUrl)?;
    let client_url = Url::parse(client_id).map_err(|_| UriError::InvalidUrl)?;

    // Get the scheme parts (excluding "://")
    let uri_scheme = uri_url.scheme();
    let client_host = client_url.host_str().unwrap_or("");

    // Convert client hostname to reverse domain notation
    let client_parts: Vec<&str> = client_host.split('.').collect();
    let expected_scheme = client_parts
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<&str>>()
        .join(".");

    Ok(uri_scheme == expected_scheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_https_redirect_uri() {
        let uri = OAuthRedirectUri::new("https://example.com/callback").unwrap();
        assert!(uri.is_https());
        assert!(!uri.is_loopback());
        assert!(!uri.is_private_use());
    }

    #[test]
    fn test_loopback_redirect_uri() {
        let uri = OAuthRedirectUri::new("http://127.0.0.1/callback").unwrap();
        assert!(uri.is_loopback());
        assert!(!uri.is_https());
        assert!(!uri.is_private_use());
    }

    #[test]
    fn test_private_use_redirect_uri() {
        let uri = OAuthRedirectUri::new("com.example.app:/callback").unwrap();
        assert!(uri.is_private_use());
        assert!(!uri.is_https());
        assert!(!uri.is_loopback());
    }

    #[test]
    fn test_invalid_uris() {
        assert!(OAuthRedirectUri::new("not-a-uri").is_err());
        assert!(OAuthRedirectUri::new("http://example.com").is_err()); // Non-loopback HTTP
        assert!(OAuthRedirectUri::new("com.example.app://callback").is_err()); // Invalid private-use
    }

    #[test]
    fn test_app_type_validation() {
        let https_uri = OAuthRedirectUri::new("https://example.com/callback").unwrap();
        let loopback_uri = OAuthRedirectUri::new("http://127.0.0.1/callback").unwrap();
        let private_uri = OAuthRedirectUri::new("com.example.app:/callback").unwrap();

        // Web app
        assert!(https_uri
            .validate_for_app_type(ApplicationType::Web)
            .is_ok());
        assert!(loopback_uri
            .validate_for_app_type(ApplicationType::Web)
            .is_err());
        assert!(private_uri
            .validate_for_app_type(ApplicationType::Web)
            .is_err());

        // Native app
        assert!(https_uri
            .validate_for_app_type(ApplicationType::Native)
            .is_ok());
        assert!(loopback_uri
            .validate_for_app_type(ApplicationType::Native)
            .is_ok());
        assert!(private_uri
            .validate_for_app_type(ApplicationType::Native)
            .is_ok());
    }

    #[test]
    fn test_private_use_client_validation() {
        assert!(validate_private_use_client_match(
            "com.example.app:/callback",
            "https://app.example.com/metadata"
        )
        .unwrap());

        assert!(!validate_private_use_client_match(
            "com.other.app:/callback",
            "https://app.example.com/metadata"
        )
        .unwrap());
    }

    #[test]
    fn test_display_and_conversion() {
        let uri_str = "https://example.com/callback";
        let uri = OAuthRedirectUri::new(uri_str).unwrap();

        assert_eq!(uri.to_string(), uri_str);
        assert_eq!(uri.as_ref(), uri_str);

        let parsed: OAuthRedirectUri = uri_str.parse().unwrap();
        assert_eq!(parsed, uri);
    }

    #[test]
    fn test_serialization() {
        let uri = OAuthRedirectUri::new("https://example.com/callback").unwrap();
        let serialized = serde_json::to_string(&uri).unwrap();
        let deserialized: OAuthRedirectUri = serde_json::from_str(&serialized).unwrap();
        assert_eq!(uri, deserialized);
    }
}
