use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth_types::{
    OAuthClientId, OAuthEndpointAuthMethod, OAuthGrantType, OAuthRedirectUri, OAuthResponseType,
    OAuthScope, WebUri,
};

/// OAuth Client Metadata.
///
/// This metadata describes an OAuth client's properties and capabilities
/// See OpenID Connect Registration 1.0 and RFC 7591.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientMetadata {
    /// List of redirect URIs for use in authorization requests
    pub redirect_uris: Vec<OAuthRedirectUri>,

    /// List of response types that the client can use
    #[serde(default = "default_response_types")]
    pub response_types: Vec<OAuthResponseType>,

    /// List of grant types that the client can use
    #[serde(default = "default_grant_types")]
    pub grant_types: Vec<OAuthGrantType>,

    /// OAuth scope values that the client can use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<OAuthScope>,

    /// Method used for token endpoint authentication
    #[serde(default)]
    pub token_endpoint_auth_method: OAuthEndpointAuthMethod,

    /// Algorithm used for token endpoint authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_signing_alg: Option<String>,

    /// Algorithm for UserInfo response signing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_signed_response_alg: Option<String>,

    /// Algorithm for UserInfo response encryption
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_encrypted_response_alg: Option<String>,

    /// URL for client's JWK Set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<WebUri>,

    /// Client's JSON Web Key Set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks: Option<serde_json::Value>,

    /// Type of application (web or native)
    #[serde(default)]
    pub application_type: ApplicationType,

    /// Subject type requested for responses
    #[serde(default)]
    pub subject_type: SubjectType,

    /// Algorithm for signing request objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_object_signing_alg: Option<String>,

    /// Algorithm for signing ID tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token_signed_response_alg: Option<String>,

    /// Algorithm for signing authorization responses
    #[serde(default = "default_auth_signing_alg")]
    pub authorization_signed_response_alg: String,

    /// Encryption method for authorization responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_encrypted_response_enc: Option<AuthorizationEncryption>,

    /// Algorithm for encrypting authorization responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_encrypted_response_alg: Option<String>,

    /// Client identifier (assigned by auth server)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<OAuthClientId>,

    /// Human-readable client name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,

    /// URL to client homepage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<WebUri>,

    /// URL to client policy document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<WebUri>,

    /// URL to client terms of service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_uri: Option<WebUri>,

    /// URL to client logo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<WebUri>,

    /// Default maximum authentication age in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_max_age: Option<u32>,

    /// Whether to require auth time claim in tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_auth_time: Option<bool>,

    /// Client contact emails
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Vec<String>>,

    /// Whether to bind access tokens to TLS client certificates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_client_certificate_bound_access_tokens: Option<bool>,

    /// Whether to bind access tokens to DPoP proofs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpop_bound_access_tokens: Option<bool>,

    /// Authorization details types that the client may use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_details_types: Option<Vec<String>>,
}

/// Application types for OAuth clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApplicationType {
    /// Web-based application
    Web,
    /// Native/mobile application
    Native,
}

impl Default for ApplicationType {
    fn default() -> Self {
        Self::Web
    }
}

/// Subject identifier types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubjectType {
    /// Public subject identifier
    Public,
    /// Pairwise subject identifier
    Pairwise,
}

impl Default for SubjectType {
    fn default() -> Self {
        Self::Public
    }
}

/// Authorization response encryption methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationEncryption {
    /// AES-128-CBC + HMAC SHA-256
    #[serde(rename = "A128CBC-HS256")]
    A128CbcHs256,
}

fn default_response_types() -> Vec<OAuthResponseType> {
    vec![OAuthResponseType::Code]
}

fn default_grant_types() -> Vec<OAuthGrantType> {
    vec![OAuthGrantType::AuthorizationCode]
}

fn default_auth_signing_alg() -> String {
    "RS256".to_string()
}

impl OAuthClientMetadata {
    /// Create new client metadata with required fields.
    pub fn new(redirect_uris: Vec<OAuthRedirectUri>) -> Result<Self, ClientMetadataError> {
        if redirect_uris.is_empty() {
            return Err(ClientMetadataError::NoRedirectUris);
        }

        Ok(Self {
            redirect_uris,
            response_types: default_response_types(),
            grant_types: default_grant_types(),
            scope: None,
            token_endpoint_auth_method: OAuthEndpointAuthMethod::default(),
            token_endpoint_auth_signing_alg: None,
            userinfo_signed_response_alg: None,
            userinfo_encrypted_response_alg: None,
            jwks_uri: None,
            jwks: None,
            application_type: ApplicationType::default(),
            subject_type: SubjectType::default(),
            request_object_signing_alg: None,
            id_token_signed_response_alg: None,
            authorization_signed_response_alg: default_auth_signing_alg(),
            authorization_encrypted_response_enc: None,
            authorization_encrypted_response_alg: None,
            client_id: None,
            client_name: None,
            client_uri: None,
            policy_uri: None,
            tos_uri: None,
            logo_uri: None,
            default_max_age: None,
            require_auth_time: None,
            contacts: None,
            tls_client_certificate_bound_access_tokens: None,
            dpop_bound_access_tokens: None,
            authorization_details_types: None,
        })
    }

    /// Validate the metadata according to spec requirements.
    pub fn validate(&self) -> Result<(), ClientMetadataError> {
        // Redirect URIs must not be empty
        if self.redirect_uris.is_empty() {
            return Err(ClientMetadataError::NoRedirectUris);
        }

        // If auth method requires signing key, verify alg is specified
        if matches!(
            self.token_endpoint_auth_method,
            OAuthEndpointAuthMethod::PrivateKeyJwt
        ) && self.token_endpoint_auth_signing_alg.is_none()
        {
            return Err(ClientMetadataError::MissingSigningAlgorithm);
        }

        // Other validation rules can be added here...

        Ok(())
    }

    /// Convert to JSON string.
    pub fn to_json(&self) -> Result<String, ClientMetadataError> {
        serde_json::to_string(self).map_err(ClientMetadataError::Serialization)
    }

    /// Parse from JSON string.
    pub fn from_json(json: &str) -> Result<Self, ClientMetadataError> {
        let metadata: Self =
            serde_json::from_str(json).map_err(ClientMetadataError::Deserialization)?;
        metadata.validate()?;
        Ok(metadata)
    }
}

/// Errors that can occur with client metadata.
#[derive(Debug, thiserror::Error)]
pub enum ClientMetadataError {
    #[error("At least one redirect URI is required")]
    NoRedirectUris,

    #[error("Token endpoint auth method requires signing algorithm")]
    MissingSigningAlgorithm,

    #[error("Failed to serialize metadata: {0}")]
    Serialization(#[source] serde_json::Error),

    #[error("Failed to deserialize metadata: {0}")]
    Deserialization(#[source] serde_json::Error),
}

impl fmt::Display for OAuthClientMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ClientMetadata(")?;
        if let Some(name) = &self.client_name {
            write!(f, "name={}, ", name)?;
        }
        write!(f, "type={:?}, ", self.application_type)?;
        write!(f, "redirect_uris={})", self.redirect_uris.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_redirect_uri() -> OAuthRedirectUri {
        OAuthRedirectUri::new("https://example.com/callback").unwrap()
    }

    #[test]
    fn test_new_metadata() {
        let metadata = OAuthClientMetadata::new(vec![test_redirect_uri()]).unwrap();
        assert_eq!(metadata.response_types, vec![OAuthResponseType::Code]);
        assert_eq!(
            metadata.grant_types,
            vec![OAuthGrantType::AuthorizationCode]
        );
        assert_eq!(metadata.application_type, ApplicationType::Web);
    }

    #[test]
    fn test_metadata_validation() {
        // No redirect URIs
        assert!(matches!(
            OAuthClientMetadata::new(vec![]),
            Err(ClientMetadataError::NoRedirectUris)
        ));

        // Missing signing algorithm
        let mut metadata = OAuthClientMetadata::new(vec![test_redirect_uri()]).unwrap();
        metadata.token_endpoint_auth_method = OAuthEndpointAuthMethod::PrivateKeyJwt;
        assert!(matches!(
            metadata.validate(),
            Err(ClientMetadataError::MissingSigningAlgorithm)
        ));
    }

    #[test]
    fn test_serialization() {
        let metadata = OAuthClientMetadata::new(vec![test_redirect_uri()]).unwrap();
        let json = metadata.to_json().unwrap();
        let parsed = OAuthClientMetadata::from_json(&json).unwrap();
        assert_eq!(metadata, parsed);
    }

    #[test]
    fn test_display() {
        let mut metadata = OAuthClientMetadata::new(vec![test_redirect_uri()]).unwrap();
        metadata.client_name = Some("Test Client".to_string());

        let display = metadata.to_string();
        assert!(display.contains("Test Client"));
        assert!(display.contains("Web"));
        assert!(display.contains("redirect_uris=1"));
    }

    #[test]
    fn test_defaults() {
        assert_eq!(ApplicationType::default(), ApplicationType::Web);
        assert_eq!(SubjectType::default(), SubjectType::Public);
        assert_eq!(default_auth_signing_alg(), "RS256".to_string());
    }
}
