use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth_types::{
    OAuthAuthorizationCodeGrantTokenRequest, OAuthClientCredentialsGrantTokenRequest,
    OAuthPasswordGrantTokenRequest, OAuthRefreshTokenGrantTokenRequest,
};

/// A token request that can be sent to the token endpoint.
///
/// This is a discriminated union of all possible token request types,
/// using the grant_type field to determine which type of request it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "grant_type", rename_all = "snake_case")]
pub enum OAuthTokenRequest {
    /// Authorization code grant request
    AuthorizationCode(OAuthAuthorizationCodeGrantTokenRequest),

    /// Refresh token grant request
    RefreshToken(OAuthRefreshTokenGrantTokenRequest),

    /// Resource owner password credentials grant request
    Password(OAuthPasswordGrantTokenRequest),

    /// Client credentials grant request
    ClientCredentials(OAuthClientCredentialsGrantTokenRequest),
}

impl OAuthTokenRequest {
    /// Returns true if this is an authorization code grant request.
    pub fn is_authorization_code(&self) -> bool {
        matches!(self, Self::AuthorizationCode(_))
    }

    /// Returns true if this is a refresh token grant request.
    pub fn is_refresh_token(&self) -> bool {
        matches!(self, Self::RefreshToken(_))
    }

    /// Returns true if this is a password grant request.
    pub fn is_password(&self) -> bool {
        matches!(self, Self::Password(_))
    }

    /// Returns true if this is a client credentials grant request.
    pub fn is_client_credentials(&self) -> bool {
        matches!(self, Self::ClientCredentials(_))
    }

    /// Get the authorization code request details if this is an authorization code grant.
    pub fn as_authorization_code(&self) -> Option<&OAuthAuthorizationCodeGrantTokenRequest> {
        match self {
            Self::AuthorizationCode(req) => Some(req),
            _ => None,
        }
    }

    /// Get the refresh token request details if this is a refresh token grant.
    pub fn as_refresh_token(&self) -> Option<&OAuthRefreshTokenGrantTokenRequest> {
        match self {
            Self::RefreshToken(req) => Some(req),
            _ => None,
        }
    }

    /// Get the password grant request details if this is a password grant.
    pub fn as_password(&self) -> Option<&OAuthPasswordGrantTokenRequest> {
        match self {
            Self::Password(req) => Some(req),
            _ => None,
        }
    }

    /// Get the client credentials request details if this is a client credentials grant.
    pub fn as_client_credentials(&self) -> Option<&OAuthClientCredentialsGrantTokenRequest> {
        match self {
            Self::ClientCredentials(req) => Some(req),
            _ => None,
        }
    }
}

/// Errors that can occur when working with token requests.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenRequestError {
    #[error("Missing grant_type parameter")]
    MissingGrantType,

    #[error("Invalid grant type: {0}")]
    InvalidGrantType(String),

    #[error("Missing required parameter: {0}")]
    MissingParameter(&'static str),

    #[error("Invalid form encoding")]
    InvalidFormEncoding,

    #[error("Authorization code error: {0}")]
    AuthorizationCodeError(#[from] crate::oauth_types::AuthorizationCodeGrantError),

    #[error("Refresh token error: {0}")]
    RefreshTokenError(#[from] crate::oauth_types::RefreshTokenGrantError),

    #[error("Password grant error: {0}")]
    PasswordGrantError(#[from] crate::oauth_types::PasswordGrantError),
}

impl fmt::Display for OAuthTokenRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorizationCode(_) => write!(f, "AuthorizationCodeGrant"),
            Self::RefreshToken(_) => write!(f, "RefreshTokenGrant"),
            Self::Password(_) => write!(f, "PasswordGrant"),
            Self::ClientCredentials(_) => write!(f, "ClientCredentialsGrant"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create test authorization code request
    fn test_auth_code_request() -> OAuthAuthorizationCodeGrantTokenRequest {
        OAuthAuthorizationCodeGrantTokenRequest::new(
            "test_code",
            "https://example.com/callback",
            Some("test_verifier"),
        )
        .unwrap()
    }

    #[test]
    fn test_request_types() {
        let auth_code = OAuthTokenRequest::AuthorizationCode(test_auth_code_request());
        assert!(auth_code.is_authorization_code());
        assert!(auth_code.as_authorization_code().is_some());
        assert!(!auth_code.is_refresh_token());
        assert!(auth_code.as_refresh_token().is_none());
    }

    #[test]
    fn test_serialization() {
        let request = OAuthTokenRequest::AuthorizationCode(test_auth_code_request());

        let json = serde_json::to_string(&request).unwrap();
        let parsed: OAuthTokenRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request, parsed);
    }

    #[test]
    fn test_display() {
        let request = OAuthTokenRequest::AuthorizationCode(test_auth_code_request());
        assert_eq!(request.to_string(), "AuthorizationCodeGrant");
    }
}
