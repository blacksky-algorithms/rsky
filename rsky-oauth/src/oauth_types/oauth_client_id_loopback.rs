use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use url::Url;

use crate::oauth_types::{OAuthClientId, OAuthRedirectUri, OAuthScope};

const PREFIX: &str = "http://localhost";

/// A loopback client ID for local OAuth flows.
///
/// These are special client IDs for localhost development and testing.
/// They must start with "http://localhost" and may include optional
/// scope and redirect_uri query parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientIdLoopback {
    /// The full client ID string
    client_id: String,
    /// Parsed configuration
    config: LoopbackConfig,
}

/// Configuration parsed from a loopback client ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopbackConfig {
    /// Optional OAuth scope
    pub scope: Option<OAuthScope>,
    /// Optional redirect URIs
    pub redirect_uris: Option<Vec<OAuthRedirectUri>>,
}

impl OAuthClientIdLoopback {
    /// Create a new loopback client ID.
    pub fn new(client_id: impl Into<String>) -> Result<Self, LoopbackClientIdError> {
        let client_id = client_id.into();

        // Must start with the prefix
        if !client_id.starts_with(PREFIX) {
            return Err(LoopbackClientIdError::InvalidPrefix);
        }

        // Parse as URL to validate format
        let url = Url::parse(&client_id).map_err(|_| LoopbackClientIdError::InvalidUrl)?;

        // Check for hash
        if url.fragment().is_some() {
            return Err(LoopbackClientIdError::ContainsHash);
        }

        // Get the starting index for path or query
        let start_idx =
            if client_id.len() > PREFIX.len() && client_id.as_bytes()[PREFIX.len()] == b'/' {
                PREFIX.len() + 1
            } else {
                PREFIX.len()
            };

        // No content after prefix is valid
        if client_id.len() == start_idx {
            return Ok(Self {
                client_id,
                config: LoopbackConfig {
                    scope: None,
                    redirect_uris: None,
                },
            });
        }

        // Must start with ? if there's more content
        if client_id.as_bytes()[start_idx] != b'?' {
            return Err(LoopbackClientIdError::InvalidPathComponent);
        }

        // Parse query parameters
        let query = &client_id[start_idx + 1..];
        let config = parse_query_params(query)?;

        Ok(Self { client_id, config })
    }

    /// Get the underlying client ID string.
    pub fn as_str(&self) -> &str {
        &self.client_id
    }

    /// Get the parsed configuration.
    pub fn config(&self) -> &LoopbackConfig {
        &self.config
    }
}

/// Parse query parameters from the client ID.
fn parse_query_params(query: &str) -> Result<LoopbackConfig, LoopbackClientIdError> {
    use url::form_urlencoded;

    let mut scope = None;
    let mut redirect_uris = Vec::new();

    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "scope" => {
                if scope.is_some() {
                    return Err(LoopbackClientIdError::DuplicateScope);
                }
                scope = Some(
                    OAuthScope::new(&*value).map_err(|_| LoopbackClientIdError::InvalidScope)?,
                );
            }
            "redirect_uri" => {
                let uri = OAuthRedirectUri::new(&*value)
                    .map_err(|_| LoopbackClientIdError::InvalidRedirectUri)?;
                redirect_uris.push(uri);
            }
            _ => {
                return Err(LoopbackClientIdError::InvalidQueryParameter(
                    key.into_owned(),
                ))
            }
        }
    }

    Ok(LoopbackConfig {
        scope,
        redirect_uris: if redirect_uris.is_empty() {
            None
        } else {
            Some(redirect_uris)
        },
    })
}

impl AsRef<str> for OAuthClientIdLoopback {
    fn as_ref(&self) -> &str {
        &self.client_id
    }
}

impl fmt::Display for OAuthClientIdLoopback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.client_id.fmt(f)
    }
}

impl FromStr for OAuthClientIdLoopback {
    type Err = LoopbackClientIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl From<OAuthClientIdLoopback> for OAuthClientId {
    fn from(loopback: OAuthClientIdLoopback) -> Self {
        // We can unwrap here because we know it's a valid client ID
        OAuthClientId::new(loopback.client_id).unwrap()
    }
}

/// Errors that can occur with loopback client IDs.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LoopbackClientIdError {
    #[error("Loopback ClientID must start with {PREFIX}")]
    InvalidPrefix,

    #[error("Invalid URL format")]
    InvalidUrl,

    #[error("Client ID must not contain a hash component")]
    ContainsHash,

    #[error("Client ID must not contain a path component")]
    InvalidPathComponent,

    #[error("Invalid query parameter: {0}")]
    InvalidQueryParameter(String),

    #[error("Invalid scope parameter")]
    InvalidScope,

    #[error("Invalid redirect URI")]
    InvalidRedirectUri,

    #[error("Duplicate scope parameter")]
    DuplicateScope,
}

/// Check if a client ID is a loopback client ID.
pub fn is_oauth_client_id_loopback(client_id: &OAuthClientId) -> bool {
    OAuthClientIdLoopback::new(client_id.to_string().as_str()).is_ok()
}

/// Assert that a client ID is a loopback client ID.
pub fn assert_oauth_loopback_client_id(client_id: &str) -> Result<(), LoopbackClientIdError> {
    OAuthClientIdLoopback::new(client_id).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_loopback() {
        let id = OAuthClientIdLoopback::new("http://localhost").unwrap();
        assert_eq!(id.as_str(), "http://localhost");
        assert!(id.config().scope.is_none());
        assert!(id.config().redirect_uris.is_none());
    }

    #[test]
    fn test_with_trailing_slash() {
        let id = OAuthClientIdLoopback::new("http://localhost/").unwrap();
        assert_eq!(id.as_str(), "http://localhost/");
        assert!(id.config().scope.is_none());
        assert!(id.config().redirect_uris.is_none());
    }

    #[test]
    fn test_invalid_prefix() {
        assert!(matches!(
            OAuthClientIdLoopback::new("https://localhost"),
            Err(LoopbackClientIdError::InvalidPrefix)
        ));
    }

    #[test]
    fn test_invalid_path() {
        assert!(matches!(
            OAuthClientIdLoopback::new("http://localhost/path"),
            Err(LoopbackClientIdError::InvalidPathComponent)
        ));
    }

    #[test]
    fn test_with_hash() {
        assert!(matches!(
            OAuthClientIdLoopback::new("http://localhost#hash"),
            Err(LoopbackClientIdError::ContainsHash)
        ));
    }

    #[test]
    fn test_invalid_query_param() {
        assert!(matches!(
            OAuthClientIdLoopback::new("http://localhost?invalid=true"),
            Err(LoopbackClientIdError::InvalidQueryParameter(_))
        ));
    }

    #[test]
    fn test_duplicate_scope() {
        assert!(matches!(
            OAuthClientIdLoopback::new("http://localhost?scope=read&scope=write"),
            Err(LoopbackClientIdError::DuplicateScope)
        ));
    }

    #[test]
    fn test_display() {
        let id = OAuthClientIdLoopback::new("http://localhost").unwrap();
        assert_eq!(id.to_string(), "http://localhost");
    }

    #[test]
    fn test_from_str() {
        let id: OAuthClientIdLoopback = "http://localhost".parse().unwrap();
        assert_eq!(id.as_str(), "http://localhost");

        let result: Result<OAuthClientIdLoopback, _> = "invalid".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_helper_functions() {
        assert!(is_oauth_client_id_loopback(
            &OAuthClientId::new("http://localhost").unwrap()
        ));
        assert!(!is_oauth_client_id_loopback(
            &OAuthClientId::new("https://example.com").unwrap()
        ));

        assert!(assert_oauth_loopback_client_id("http://localhost").is_ok());
        assert!(assert_oauth_loopback_client_id("invalid").is_err());
    }

    // Note: Additional tests for scope and redirect_uri parsing would be added
    // once those types are fully implemented
}
