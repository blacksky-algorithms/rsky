use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth_types::{OAuthCodeChallengeMethod, OAuthGrantType, OAuthIssuerIdentifier, WebUri};

/// OAuth Authorization Server Metadata.
///
/// This metadata describes an OAuth 2.0 authorization server's configuration,
/// including all necessary endpoints and supported features.
/// See RFC 8414 for details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthAuthorizationServerMetadata {
    /// Authorization server's issuer identifier URL
    pub issuer: OAuthIssuerIdentifier,

    /// Supported OpenID Connect claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claims_supported: Option<Vec<String>>,

    /// Supported claim locales
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claims_locales_supported: Option<Vec<String>>,

    /// Whether the claims parameter is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claims_parameter_supported: Option<bool>,

    /// Whether the request parameter is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_parameter_supported: Option<bool>,

    /// Whether the request_uri parameter is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_uri_parameter_supported: Option<bool>,

    /// Whether registration of request_uri values is required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_request_uri_registration: Option<bool>,

    /// Supported scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,

    /// Supported subject identifier types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_types_supported: Option<Vec<String>>,

    /// Supported response types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_types_supported: Option<Vec<String>>,

    /// Supported response modes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modes_supported: Option<Vec<String>>,

    /// Supported grant types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types_supported: Option<Vec<OAuthGrantType>>,

    /// Supported PKCE challenge methods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_challenge_methods_supported: Option<Vec<OAuthCodeChallengeMethod>>,

    /// Supported UI locales
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_locales_supported: Option<Vec<String>>,

    /// Supported ID Token signing algorithms
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token_signing_alg_values_supported: Option<Vec<String>>,

    /// Supported display values for the authorization UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_values_supported: Option<Vec<String>>,

    /// Supported signing algorithms for request objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_object_signing_alg_values_supported: Option<Vec<String>>,

    /// Whether authorization_response_iss parameter is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_response_iss_parameter_supported: Option<bool>,

    /// Supported authorization details types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_details_types_supported: Option<Vec<String>>,

    /// Supported encryption algorithms for request objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_object_encryption_alg_values_supported: Option<Vec<String>>,

    /// Supported encryption encodings for request objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_object_encryption_enc_values_supported: Option<Vec<String>>,

    /// URL of the authorization server's JWK Set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<WebUri>,

    /// Authorization endpoint URL
    pub authorization_endpoint: WebUri,

    /// Token endpoint URL
    pub token_endpoint: WebUri,

    /// Supported token endpoint authentication methods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,

    /// Supported signing algorithms for token endpoint authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_signing_alg_values_supported: Option<Vec<String>>,

    /// Token revocation endpoint URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<WebUri>,

    /// Token introspection endpoint URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub introspection_endpoint: Option<WebUri>,

    /// Pushed authorization request endpoint URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pushed_authorization_request_endpoint: Option<WebUri>,

    /// Whether pushed authorization requests are required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_pushed_authorization_requests: Option<bool>,

    /// UserInfo endpoint URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_endpoint: Option<WebUri>,

    /// End session endpoint URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_session_endpoint: Option<WebUri>,

    /// Dynamic registration endpoint URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<WebUri>,

    /// Supported DPoP signing algorithms
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpop_signing_alg_values_supported: Option<Vec<String>>,

    /// Protected resource URLs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protected_resources: Option<Vec<WebUri>>,

    /// Whether client ID metadata discovery is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id_metadata_document_supported: Option<bool>,
}

impl OAuthAuthorizationServerMetadata {
    /// Create a new authorization server metadata instance.
    pub fn new(
        issuer: OAuthIssuerIdentifier,
        authorization_endpoint: WebUri,
        token_endpoint: WebUri,
    ) -> Self {
        Self {
            issuer,
            authorization_endpoint,
            token_endpoint,
            claims_supported: None,
            claims_locales_supported: None,
            claims_parameter_supported: None,
            request_parameter_supported: None,
            request_uri_parameter_supported: None,
            require_request_uri_registration: None,
            scopes_supported: None,
            subject_types_supported: None,
            response_types_supported: None,
            response_modes_supported: None,
            grant_types_supported: None,
            code_challenge_methods_supported: None,
            ui_locales_supported: None,
            id_token_signing_alg_values_supported: None,
            display_values_supported: None,
            request_object_signing_alg_values_supported: None,
            authorization_response_iss_parameter_supported: None,
            authorization_details_types_supported: None,
            request_object_encryption_alg_values_supported: None,
            request_object_encryption_enc_values_supported: None,
            jwks_uri: None,
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            revocation_endpoint: None,
            introspection_endpoint: None,
            pushed_authorization_request_endpoint: None,
            require_pushed_authorization_requests: None,
            userinfo_endpoint: None,
            end_session_endpoint: None,
            registration_endpoint: None,
            dpop_signing_alg_values_supported: None,
            protected_resources: None,
            client_id_metadata_document_supported: None,
        }
    }

    /// Validate the metadata configuration.
    pub fn validate(&self) -> Result<(), MetadataError> {
        // Required validations per RFC 8414

        // If PAR is required, PAR endpoint must be present
        if self.require_pushed_authorization_requests == Some(true)
            && self.pushed_authorization_request_endpoint.is_none()
        {
            return Err(MetadataError::MissingParEndpoint);
        }

        // Response types must include "code" if specified
        if let Some(ref types) = self.response_types_supported {
            if !types.iter().any(|t| t == "code") {
                return Err(MetadataError::CodeResponseTypeRequired);
            }
        }

        // Additional validations that would be required by the spec
        // can be added here

        Ok(())
    }
}

impl fmt::Display for OAuthAuthorizationServerMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AuthorizationServer(issuer={})", self.issuer)
    }
}

/// Errors that can occur when validating metadata.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MetadataError {
    #[error("PAR endpoint required when PAR is required")]
    MissingParEndpoint,

    #[error("Response type \"code\" is required")]
    CodeResponseTypeRequired,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_types::uri::ValidUri;

    // Helper function to create test issuer
    fn test_issuer() -> OAuthIssuerIdentifier {
        OAuthIssuerIdentifier::new("https://example.com").unwrap()
    }

    // Helper function to create test URI
    fn test_uri() -> WebUri {
        WebUri::validate("https://example.com").unwrap()
    }

    #[test]
    fn test_new() {
        let issuer = test_issuer();
        let auth_endpoint = test_uri();
        let token_endpoint = test_uri();

        let metadata = OAuthAuthorizationServerMetadata::new(
            issuer.clone(),
            auth_endpoint.clone(),
            token_endpoint.clone(),
        );

        assert_eq!(metadata.issuer, issuer);
        assert_eq!(metadata.authorization_endpoint, auth_endpoint);
        assert_eq!(metadata.token_endpoint, token_endpoint);
    }

    #[test]
    fn test_par_validation() {
        let mut metadata =
            OAuthAuthorizationServerMetadata::new(test_issuer(), test_uri(), test_uri());

        // PAR required but no endpoint
        metadata.require_pushed_authorization_requests = Some(true);
        assert_eq!(metadata.validate(), Err(MetadataError::MissingParEndpoint));

        // PAR required with endpoint (valid)
        metadata.pushed_authorization_request_endpoint = Some(test_uri());
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_response_types_validation() {
        let mut metadata =
            OAuthAuthorizationServerMetadata::new(test_issuer(), test_uri(), test_uri());

        // Response types without "code"
        metadata.response_types_supported = Some(vec!["token".to_string()]);
        assert_eq!(
            metadata.validate(),
            Err(MetadataError::CodeResponseTypeRequired)
        );

        // Response types with "code" (valid)
        metadata.response_types_supported = Some(vec!["code".to_string(), "token".to_string()]);
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_serialization() {
        let metadata = OAuthAuthorizationServerMetadata::new(test_issuer(), test_uri(), test_uri());

        let serialized = serde_json::to_string(&metadata).unwrap();
        let deserialized: OAuthAuthorizationServerMetadata =
            serde_json::from_str(&serialized).unwrap();

        assert_eq!(metadata, deserialized);
    }

    #[test]
    fn test_display() {
        let metadata = OAuthAuthorizationServerMetadata::new(test_issuer(), test_uri(), test_uri());

        assert!(metadata.to_string().starts_with("AuthorizationServer"));
    }
}
