use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::oauth_types::{OAuthClientIdLoopback, OAuthRedirectUri, OAuthScope, OAuthScopeError};

/// Creates client metadata for a loopback client.
///
/// Loopback clients are used for local development and testing.
/// The client ID must start with "http://localhost" and can optionally
/// include query parameters for scope and redirect URIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtprotoLoopbackClientMetadata {
    /// Client identifier
    pub client_id: OAuthClientIdLoopback,
    /// OAuth scopes requested
    pub scope: OAuthScope,
    /// List of redirect URIs
    pub redirect_uris: Vec<OAuthRedirectUri>,
    /// Client name (for development)
    pub client_name: String,
    /// Response types supported
    pub response_types: Vec<String>,
    /// Grant types supported
    pub grant_types: Vec<String>,
    /// Token endpoint auth method
    pub token_endpoint_auth_method: String,
    /// Application type
    pub application_type: String,
    /// Whether DPoP tokens are required
    pub dpop_bound_access_tokens: bool,
}

impl AtprotoLoopbackClientMetadata {
    /// Create new loopback client metadata.
    pub fn new(client_id: OAuthClientIdLoopback) -> Result<Self, ClientError> {
        // Parse config from client ID
        let config = client_id.config();

        // Get scopes, defaulting to "atproto"
        let scope = config
            .scope
            .clone()
            .unwrap_or_else(|| OAuthScope::new("atproto").expect("default scope is valid"));

        // Get redirect URIs with defaults
        let redirect_uris = config.redirect_uris.clone().unwrap_or_else(|| {
            vec![
                OAuthRedirectUri::new("http://127.0.0.1/").expect("default URI is valid"),
                OAuthRedirectUri::new("http://[::1]/").expect("default URI is valid"),
            ]
        });

        // Validate redirect URIs are not duplicates
        let uri_set: HashSet<_> = HashSet::from_iter(redirect_uris.iter());
        if uri_set.len() != redirect_uris.len() {
            return Err(ClientError::DuplicateRedirectUri);
        }

        Ok(Self {
            client_id,
            scope,
            redirect_uris,
            client_name: "Loopback client".to_string(),
            response_types: vec!["code".to_string()],
            grant_types: vec![
                "authorization_code".to_string(),
                "refresh_token".to_string(),
            ],
            // token_endpoint_auth_signing_alg (string, optional): none is never allowed here.
            // The current recommended and most-supported algorithm is ES256,
            // but this may evolve over time.
            // Authorization Servers will compare this against their supported algorithms.
            token_endpoint_auth_method: "ES256".to_string(),
            application_type: "native".to_string(),
            dpop_bound_access_tokens: true,
        })
    }

    /// Get the client ID.
    pub fn client_id(&self) -> &OAuthClientIdLoopback {
        &self.client_id
    }

    /// Get the scopes.
    pub fn scope(&self) -> &OAuthScope {
        &self.scope
    }

    /// Get the redirect URIs.
    pub fn redirect_uris(&self) -> &[OAuthRedirectUri] {
        &self.redirect_uris
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Invalid OAuth scope")]
    Scope(#[from] OAuthScopeError),

    #[error("Duplicate redirect URI")]
    DuplicateRedirectUri,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client_id() -> OAuthClientIdLoopback {
        OAuthClientIdLoopback::new("http://localhost").unwrap()
    }

    #[test]
    fn test_new_metadata() {
        let metadata = AtprotoLoopbackClientMetadata::new(test_client_id()).unwrap();

        assert_eq!(metadata.scope().as_ref(), "atproto");
        assert_eq!(metadata.redirect_uris().len(), 2);
        assert_eq!(metadata.client_name, "Loopback client");
        assert_eq!(metadata.response_types, vec!["code"]);
        assert!(metadata.dpop_bound_access_tokens);
    }

    #[test]
    fn test_duplicate_uris() {
        let client_id = OAuthClientIdLoopback::new(
            "http://localhost?redirect_uri=http://127.0.0.1&redirect_uri=http://127.0.0.1",
        )
        .unwrap();

        assert!(matches!(
            AtprotoLoopbackClientMetadata::new(client_id),
            Err(ClientError::DuplicateRedirectUri)
        ));
    }

    #[test]
    fn test_serialization() {
        let metadata = AtprotoLoopbackClientMetadata::new(test_client_id()).unwrap();

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: AtprotoLoopbackClientMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata, deserialized);
    }
}
