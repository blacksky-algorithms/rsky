use crate::oauth_types::util::{is_hostname_ip, is_loopback_host};
use http::Uri;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UriError {
    #[error("Invalid URL")]
    InvalidUrl,
    #[error("URL must use the {0} protocol")]
    InvalidProtocol(String),
    #[error("URL must use \"localhost\", \"127.0.0.1\" or \"[::1]\" as hostname")]
    InvalidLoopbackHost,
    #[error("https: URL must not use a loopback host")]
    HttpsLoopbackNotAllowed,
    #[error("Domain name must contain at least two segments")]
    InvalidDomainSegments,
    #[error("Domain name must not end with \".local\"")]
    LocalDomainNotAllowed,
    #[error("Private-use URI scheme requires a \".\" as part of the protocol")]
    InvalidPrivateUseScheme,
    #[error("Private-use URI schemes must not include a hostname (only one \"/\" is allowed after the protocol, as per RFC 8252)")]
    PrivateUseHostnameNotAllowed,
}

/// A trait for URI validation
pub trait ValidUri: Sized {
    fn validate(uri_str: &str) -> Result<Self, UriError>;
}

/// Valid, but potentially dangerous URL (`data:`, `file:`, `javascript:`, etc.)
#[derive(Debug, Clone)]
pub struct DangerousUri(String);

impl DangerousUri {
    /// Returns a string slice of the underlying URI
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for DangerousUri {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ValidUri for DangerousUri {
    fn validate(uri_str: &str) -> Result<Self, UriError> {
        if !uri_str.contains(':') {
            return Err(UriError::InvalidUrl);
        }

        // Try parsing as URI to validate
        Uri::from_str(uri_str).map_err(|_| UriError::InvalidUrl)?;

        Ok(DangerousUri(uri_str.to_string()))
    }
}

/// Loopback URI (http://localhost, http://127.0.0.1, http://::1)
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub struct LoopbackUri(String);

impl LoopbackUri {
    /// Returns a string slice of the underlying URI
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for LoopbackUri {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ValidUri for LoopbackUri {
    fn validate(uri_str: &str) -> Result<Self, UriError> {
        if !uri_str.starts_with("http://") {
            return Err(UriError::InvalidProtocol("http:".to_string()));
        }

        let uri = Uri::from_str(uri_str).map_err(|_| UriError::InvalidUrl)?;

        let hostname = uri.authority().ok_or(UriError::InvalidUrl)?.host();

        if !is_loopback_host(hostname) {
            return Err(UriError::InvalidLoopbackHost);
        }

        Ok(LoopbackUri(uri_str.to_string()))
    }
}

/// HTTPS URI
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub struct HttpsUri(String);

impl HttpsUri {
    /// Returns a string slice of the underlying URI
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn new(val: String) -> Self {
        Self(val)
    }
}

impl Display for HttpsUri {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ValidUri for HttpsUri {
    fn validate(uri_str: &str) -> Result<Self, UriError> {
        if !uri_str.starts_with("https://") {
            return Err(UriError::InvalidProtocol("https:".to_string()));
        }

        let uri = Uri::from_str(uri_str).map_err(|_| UriError::InvalidUrl)?;

        let hostname = uri.authority().ok_or(UriError::InvalidUrl)?.host();

        // Disallow loopback URLs with https:
        if is_loopback_host(hostname) {
            return Err(UriError::HttpsLoopbackNotAllowed);
        }

        if is_hostname_ip(hostname) {
            // Hostname is IP address - allowed
        } else {
            // Hostname is domain name
            if !hostname.contains('.') {
                return Err(UriError::InvalidDomainSegments);
            }

            if hostname.ends_with(".local") {
                return Err(UriError::LocalDomainNotAllowed);
            }
        }

        Ok(HttpsUri(uri_str.to_string()))
    }
}

/// Web URI (either LoopbackUri or HttpsUri)
#[derive(Debug, Clone, PartialEq, Ord, Eq, Serialize, Deserialize, PartialOrd)]
pub enum WebUri {
    Loopback(LoopbackUri),
    Https(HttpsUri),
}

impl ValidUri for WebUri {
    fn validate(uri_str: &str) -> Result<Self, UriError> {
        if uri_str.starts_with("http://") {
            LoopbackUri::validate(uri_str).map(WebUri::Loopback)
        } else if uri_str.starts_with("https://") {
            HttpsUri::validate(uri_str).map(WebUri::Https)
        } else {
            Err(UriError::InvalidProtocol("http: or https:".to_string()))
        }
    }
}

impl Display for WebUri {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loopback(uri) => write!(f, "{}", uri.0),
            Self::Https(uri) => write!(f, "{}", uri.0),
        }
    }
}

impl WebUri {
    pub fn domain(&self) -> String {
        match self {
            WebUri::Loopback(_) => String::from("loopback"),
            WebUri::Https(_) => String::from("placeholder"),
        }
    }
}

/// Private-use URI (custom scheme with dot)
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateUseUri(String);

impl PrivateUseUri {
    /// Returns a string slice of the underlying URI
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for PrivateUseUri {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ValidUri for PrivateUseUri {
    fn validate(uri_str: &str) -> Result<Self, UriError> {
        // Check for colon, which separates scheme from path
        let colon_idx = uri_str.find(':').ok_or(UriError::InvalidUrl)?;
        let scheme = &uri_str[..colon_idx];

        // Protocol (scheme) must contain a dot per RFC 7595
        if !scheme.contains('.') {
            return Err(UriError::InvalidPrivateUseScheme);
        }

        // Check for scheme validity (shouldn't start or end with dot, etc.)
        if scheme.starts_with('.') || scheme.ends_with('.') || scheme.contains("..") {
            return Err(UriError::InvalidPrivateUseScheme);
        }

        // Check path part (after the colon)
        let path_part = &uri_str[colon_idx + 1..];

        // According to RFC 8252, there must be exactly one slash after the scheme
        if !path_part.starts_with('/') {
            return Err(UriError::InvalidUrl);
        }

        // Check if there's more than one slash (which would indicate a hostname)
        if path_part.len() > 1 && path_part[1..].contains('/') {
            return Err(UriError::PrivateUseHostnameNotAllowed);
        }

        Ok(PrivateUseUri(uri_str.to_string()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_uri_as_str() {
        let loopback = LoopbackUri::validate("http://localhost").unwrap();
        assert_eq!(loopback.as_str(), "http://localhost");

        let https = HttpsUri::validate("https://example.com").unwrap();
        assert_eq!(https.as_str(), "https://example.com");

        let web_loopback = WebUri::validate("http://localhost").unwrap();
        assert_eq!(web_loopback.to_string(), String::from("http://localhost"));

        let web_https = WebUri::validate("https://example.com").unwrap();
        assert_eq!(web_https.to_string(), String::from("https://example.com"));
    }

    use super::*;

    #[test]
    fn test_dangerous_uri() {
        let valid_uris = vec![
            "javascript:alert(1)",
            "https://example.com/payments",
            "https://example.com/accounts",
            "https://signing.example.com/signdoc",
            "https://taxservice.govehub.no.example.com",
            "https://fhir.example.com/patient",
            "https://server.example.net/",
            "https://resource.local/other",
        ];
        for uri in valid_uris {
            assert!(
                DangerousUri::validate(uri).is_ok(),
                "URI should be valid: {}",
                uri
            );
        }
    }

    #[test]
    fn test_invalid_dangerous_uri() {
        let invalid_uris = vec![
            // Missing scheme
            "example.com/payments",
            "//example.com/accounts",
            // Invalid characters
            "https://example.com/pay ments",
            "https://exam ple.com/accounts",
            // Empty strings
            "",
            " ",
            // Malformed URLs
            "http://",
            "https://",
            "http:///example.com",
            // Invalid schemes
            "file:/etc/passwd",
            // Non-ASCII characters in hostname
            "https://exämple.com/path",
            // Missing host
            "https:///path",
            // Backslashes instead of forward slashes
            "https://example.com\\path",
        ];

        for uri in invalid_uris {
            assert!(
                DangerousUri::validate(uri).is_err(),
                "URI should be invalid: {}",
                uri
            );
        }
    }

    #[test]
    fn test_loopback_uri() {
        assert!(LoopbackUri::validate("http://localhost").is_ok());
        assert!(LoopbackUri::validate("http://127.0.0.1").is_ok());
        assert!(LoopbackUri::validate("http://[::1]").is_ok());
        assert!(LoopbackUri::validate("https://localhost").is_err());
        assert!(LoopbackUri::validate("http://example.com").is_err());
    }

    #[test]
    fn test_https_uri() {
        assert!(HttpsUri::validate("https://example.com").is_ok());
        assert!(HttpsUri::validate("https://192.168.1.1").is_ok());
        assert!(HttpsUri::validate("https://localhost").is_err());
        assert!(HttpsUri::validate("https://example").is_err());
        assert!(HttpsUri::validate("https://test.local").is_err());
    }

    #[test]
    fn test_web_uri() {
        assert!(matches!(
            WebUri::validate("http://localhost"),
            Ok(WebUri::Loopback(_))
        ));
        assert!(matches!(
            WebUri::validate("https://example.com"),
            Ok(WebUri::Https(_))
        ));
        assert!(WebUri::validate("ftp://example.com").is_err());
    }

    #[test]
    fn test_private_use_uri() {
        // Valid cases
        let valid_cases = vec![
            "com.example.app:/callback",
            "org.example.myapp:/oauth",
            "io.github.project:/redirect",
            "net.example.service:/oauth2callback",
        ];

        for uri in valid_cases {
            assert!(
                PrivateUseUri::validate(uri).is_ok(),
                "URI should be valid: {}",
                uri
            );
        }

        // Invalid cases with expected errors
        let invalid_cases = vec![
            // (URI string, Expected error, Description)
            (
                "example:path",
                UriError::InvalidPrivateUseScheme,
                "no dot in protocol",
            ),
            (
                "example:/path.test",
                UriError::InvalidPrivateUseScheme,
                "no dot in protocol",
            ),
            (
                "com.example://path",
                UriError::PrivateUseHostnameNotAllowed,
                "more than one slash (indicates hostname)",
            ),
            (
                "com.example:/path/subpath",
                UriError::PrivateUseHostnameNotAllowed,
                "multiple path segments not allowed",
            ),
            ("", UriError::InvalidUrl, "empty string"),
            ("not a url", UriError::InvalidUrl, "invalid URL format"),
            (
                ".example:/path",
                UriError::InvalidPrivateUseScheme,
                "leading dot in protocol",
            ),
            (
                "com.example.app:callback",
                UriError::InvalidUrl,
                "missing slash after colon",
            ),
        ];

        for (uri, expected_error, description) in invalid_cases {
            assert!(
                matches!(PrivateUseUri::validate(uri), Err(err) if err == expected_error),
                "URI '{}' ({}) did not fail with expected error {:?}",
                uri,
                description,
                expected_error
            );
        }
    }
}
