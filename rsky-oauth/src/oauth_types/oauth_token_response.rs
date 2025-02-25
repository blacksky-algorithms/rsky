use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth_types::{
    OAuthAccessToken, OAuthAuthorizationDetails, OAuthRefreshToken, OAuthTokenType,
};

use jsonwebtoken::TokenData;

/// Success response from a token endpoint.
///
/// See RFC 6749 section 5.1 and OpenID Connect Core for response details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthTokenResponse {
    /// The access token string
    pub access_token: OAuthAccessToken,

    /// Type of access token issued
    pub token_type: OAuthTokenType,

    /// Granted scopes (required if different from requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Refresh token that can be used to obtain new access tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<OAuthRefreshToken>,

    /// Access token expiration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u32>,

    /// ID Token for OpenID Connect flows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>, // Signed JWT

    /// Authorization details granted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_details: Option<OAuthAuthorizationDetails>,

    /// Additional fields not defined in base OAuth spec
    #[serde(flatten)]
    pub additional_fields: std::collections::HashMap<String, serde_json::Value>,
}

impl OAuthTokenResponse {
    /// Create a new token response.
    ///
    /// The minimum required fields are access_token and token_type.
    pub fn new(access_token: OAuthAccessToken, token_type: OAuthTokenType) -> Self {
        Self {
            access_token,
            token_type,
            scope: None,
            refresh_token: None,
            expires_in: None,
            id_token: None,
            authorization_details: None,
            additional_fields: std::collections::HashMap::new(),
        }
    }

    /// Set the granted scopes.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Set the refresh token.
    pub fn with_refresh_token(mut self, refresh_token: OAuthRefreshToken) -> Self {
        self.refresh_token = Some(refresh_token);
        self
    }

    /// Set the access token expiration time in seconds.
    pub fn with_expires_in(mut self, expires_in: u32) -> Self {
        self.expires_in = Some(expires_in);
        self
    }

    /// Set the ID token.
    pub fn with_id_token(mut self, id_token: impl Into<String>) -> Self {
        self.id_token = Some(id_token.into());
        self
    }

    /// Set the authorization details.
    pub fn with_authorization_details(mut self, details: OAuthAuthorizationDetails) -> Self {
        self.authorization_details = Some(details);
        self
    }

    /// Add an additional field to the response.
    pub fn with_additional_field(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.additional_fields.insert(key.into(), value.into());
        self
    }

    /// Returns true if this response includes an ID token (OpenID Connect).
    pub fn has_id_token(&self) -> bool {
        self.id_token.is_some()
    }

    /// Returns true if this response includes a refresh token.
    pub fn has_refresh_token(&self) -> bool {
        self.refresh_token.is_some()
    }

    /// Parse the ID token if present and validate its signature.
    pub fn parse_id_token<T>(&self, key: &[u8]) -> Result<Option<TokenData<T>>, TokenResponseError>
    where
        T: for<'a> Deserialize<'a>,
    {
        match &self.id_token {
            Some(token) => {
                let decoded = jsonwebtoken::decode(
                    token,
                    &jsonwebtoken::DecodingKey::from_secret(key),
                    &jsonwebtoken::Validation::default(),
                )
                .map_err(TokenResponseError::InvalidIdToken)?;
                Ok(Some(decoded))
            }
            None => Ok(None),
        }
    }

    /// Convert to JSON string.
    pub fn to_json(&self) -> Result<String, TokenResponseError> {
        serde_json::to_string(self).map_err(TokenResponseError::Serialization)
    }

    /// Parse from JSON string.
    pub fn from_json(json: &str) -> Result<Self, TokenResponseError> {
        serde_json::from_str(json).map_err(TokenResponseError::Deserialization)
    }
}

/// Errors that can occur when working with token responses.
#[derive(Debug, thiserror::Error)]
pub enum TokenResponseError {
    #[error("Failed to serialize token response: {0}")]
    Serialization(#[source] serde_json::Error),

    #[error("Failed to deserialize token response: {0}")]
    Deserialization(#[source] serde_json::Error),

    #[error("Invalid ID token: {0}")]
    InvalidIdToken(#[source] jsonwebtoken::errors::Error),
}

impl fmt::Display for OAuthTokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TokenResponse(type={}, ", self.token_type)?;
        if let Some(scope) = &self.scope {
            write!(f, "scope={}, ", scope)?;
        }
        if self.has_refresh_token() {
            write!(f, "has_refresh=true, ")?;
        }
        if let Some(expires_in) = self.expires_in {
            write!(f, "expires_in={}s, ", expires_in)?;
        }
        write!(f, "token={})", self.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_access_token() -> OAuthAccessToken {
        OAuthAccessToken::new("test_access_token").unwrap()
    }

    fn test_refresh_token() -> OAuthRefreshToken {
        OAuthRefreshToken::new("test_refresh_token").unwrap()
    }

    #[test]
    fn test_basic_response() {
        let response = OAuthTokenResponse::new(test_access_token(), OAuthTokenType::Bearer);
        assert_eq!(response.token_type, OAuthTokenType::Bearer);
        assert!(!response.has_refresh_token());
        assert!(!response.has_id_token());
    }

    #[test]
    fn test_full_response() {
        let response = OAuthTokenResponse::new(test_access_token(), OAuthTokenType::Bearer)
            .with_scope("read write")
            .with_refresh_token(test_refresh_token())
            .with_expires_in(3600)
            .with_id_token("test.jwt.token");

        assert!(response.has_refresh_token());
        assert!(response.has_id_token());
        assert_eq!(response.scope, Some("read write".to_string()));
        assert_eq!(response.expires_in, Some(3600));
    }

    #[test]
    fn test_additional_fields() {
        let response = OAuthTokenResponse::new(test_access_token(), OAuthTokenType::Bearer)
            .with_additional_field("custom_field", "custom_value");

        assert_eq!(
            response.additional_fields.get("custom_field").unwrap(),
            &serde_json::Value::String("custom_value".to_string())
        );
    }

    #[test]
    fn test_serialization() {
        let response = OAuthTokenResponse::new(test_access_token(), OAuthTokenType::Bearer)
            .with_scope("read")
            .with_expires_in(3600);

        let json = response.to_json().unwrap();
        let parsed = OAuthTokenResponse::from_json(&json).unwrap();
        assert_eq!(response, parsed);
    }

    #[test]
    fn test_display() {
        let response = OAuthTokenResponse::new(test_access_token(), OAuthTokenType::Bearer)
            .with_scope("read")
            .with_expires_in(3600)
            .with_refresh_token(test_refresh_token());

        let display = response.to_string();
        assert!(display.contains("Bearer"));
        assert!(display.contains("read"));
        assert!(display.contains("3600s"));
        assert!(display.contains("has_refresh=true"));
    }

    // Note: ID token validation tests would go here but require actual JWT examples
}
