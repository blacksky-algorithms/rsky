use crate::oauth_provider::token::refresh_token::RefreshToken;
use crate::oauth_types::{OAuthAccessToken, OAuthAuthorizationDetails, OAuthScope, OAuthTokenType};
use serde::{Deserialize, Serialize};
use std::fmt;

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
    pub scope: Option<OAuthScope>,

    /// Refresh token that can be used to obtain new access tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<RefreshToken>,

    /// Access token expiration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,

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
    pub fn with_scope(mut self, scope: OAuthScope) -> Self {
        self.scope = Some(scope);
        self
    }

    /// Set the refresh token.
    pub fn with_refresh_token(mut self, refresh_token: RefreshToken) -> Self {
        self.refresh_token = Some(refresh_token);
        self
    }

    /// Set the access token expiration time in seconds.
    pub fn with_expires_in(mut self, expires_in: i64) -> Self {
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

    // /// Parse the ID token if present and validate its signature.
    // pub fn parse_id_token<T>(&self, key: &[u8]) -> Result<Option<TokenData<T>>, TokenResponseError>
    // where
    //     T: for<'a> Deserialize<'a>,
    // {
    //     match &self.id_token {
    //         Some(token) => {
    //             let decoded = jsonwebtoken::decode(
    //                 token,
    //                 &jsonwebtoken::DecodingKey::from_secret(key),
    //                 &jsonwebtoken::Validation::default(),
    //             )
    //             .map_err(TokenResponseError::InvalidIdToken)?;
    //             Ok(Some(decoded))
    //         }
    //         None => Ok(None),
    //     }
    // }

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

    #[error("Invalid ID tokens")]
    InvalidIdToken,
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
