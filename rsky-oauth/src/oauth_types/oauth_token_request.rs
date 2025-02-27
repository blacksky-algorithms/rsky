use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

use crate::oauth_types::{
    OAuthAuthorizationCodeGrantTokenRequest, OAuthClientCredentialsGrantTokenRequest,
    OAuthPasswordGrantTokenRequest, OAuthRefreshTokenGrantTokenRequest,
};

/// A token request that can be sent to the token endpoint.
///
/// This is a discriminated union of all possible token request types,
/// using the grant_type field to determine which type of request it is.
#[derive(Debug, Clone, PartialEq, Eq)]
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

// Custom serialization for OAuthTokenRequest
impl Serialize for OAuthTokenRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::AuthorizationCode(req) => req.serialize(serializer),
            Self::RefreshToken(req) => req.serialize(serializer),
            Self::Password(req) => req.serialize(serializer),
            Self::ClientCredentials(req) => req.serialize(serializer),
        }
    }
}

// Custom deserialization for OAuthTokenRequest
impl<'de> Deserialize<'de> for OAuthTokenRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // First, deserialize to a JSON Value to inspect the grant_type
        let value: serde_json::Value = serde_json::Value::deserialize(deserializer)?;

        // Extract the grant_type field
        let grant_type = match value.get("grant_type") {
            Some(serde_json::Value::String(gt)) => gt.as_str(),
            _ => return Err(serde::de::Error::missing_field("grant_type")),
        };

        // Reserialize the value to a string
        let json_str = serde_json::to_string(&value)
            .map_err(|e| serde::de::Error::custom(format!("JSON serialization error: {}", e)))?;

        // Deserialize to the appropriate variant based on grant_type
        match grant_type {
            "authorization_code" => {
                let req =
                    serde_json::from_str::<OAuthAuthorizationCodeGrantTokenRequest>(&json_str)
                        .map_err(|e| {
                            serde::de::Error::custom(format!(
                                "AuthorizationCode deserialization error: {}",
                                e
                            ))
                        })?;
                Ok(OAuthTokenRequest::AuthorizationCode(req))
            }
            "refresh_token" => {
                let req = serde_json::from_str::<OAuthRefreshTokenGrantTokenRequest>(&json_str)
                    .map_err(|e| {
                        serde::de::Error::custom(format!(
                            "RefreshToken deserialization error: {}",
                            e
                        ))
                    })?;
                Ok(OAuthTokenRequest::RefreshToken(req))
            }
            "password" => {
                let req = serde_json::from_str::<OAuthPasswordGrantTokenRequest>(&json_str)
                    .map_err(|e| {
                        serde::de::Error::custom(format!("Password deserialization error: {}", e))
                    })?;
                Ok(OAuthTokenRequest::Password(req))
            }
            "client_credentials" => {
                let req =
                    serde_json::from_str::<OAuthClientCredentialsGrantTokenRequest>(&json_str)
                        .map_err(|e| {
                            serde::de::Error::custom(format!(
                                "ClientCredentials deserialization error: {}",
                                e
                            ))
                        })?;
                Ok(OAuthTokenRequest::ClientCredentials(req))
            }
            _ => Err(serde::de::Error::custom(format!(
                "Invalid grant_type: {}",
                grant_type
            ))),
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
    use crate::oauth_types::OAuthRefreshToken;

    // Helper function to create test authorization code request
    fn test_auth_code_request() -> OAuthAuthorizationCodeGrantTokenRequest {
        // Create a code verifier that meets the minimum length requirement of 43 characters
        let valid_code_verifier = "1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJ-";

        OAuthAuthorizationCodeGrantTokenRequest::new(
            "test_code",
            "https://example.com/callback",
            Some(valid_code_verifier),
        )
        .unwrap()
    }

    // Helper function for password grant request
    fn test_password_request() -> OAuthPasswordGrantTokenRequest {
        OAuthPasswordGrantTokenRequest::new("testuser", "testpassword").unwrap()
    }

    // Helper function for refresh token request
    fn test_refresh_token_request() -> OAuthRefreshTokenGrantTokenRequest {
        let refresh_token = OAuthRefreshToken::new("test_refresh_token").unwrap();
        OAuthRefreshTokenGrantTokenRequest::new(refresh_token)
    }

    // Helper function for client credentials request
    fn test_client_credentials_request() -> OAuthClientCredentialsGrantTokenRequest {
        OAuthClientCredentialsGrantTokenRequest::new()
    }

    #[test]
    fn test_request_types() {
        // Test authorization code request
        let auth_code = OAuthTokenRequest::AuthorizationCode(test_auth_code_request());
        assert!(auth_code.is_authorization_code());
        assert!(auth_code.as_authorization_code().is_some());
        assert!(!auth_code.is_refresh_token());
        assert!(auth_code.as_refresh_token().is_none());

        // Test refresh token request
        let refresh = OAuthTokenRequest::RefreshToken(test_refresh_token_request());
        assert!(refresh.is_refresh_token());
        assert!(refresh.as_refresh_token().is_some());
        assert!(!refresh.is_authorization_code());

        // Test password request
        let password = OAuthTokenRequest::Password(test_password_request());
        assert!(password.is_password());
        assert!(password.as_password().is_some());
        assert!(!password.is_client_credentials());

        // Test client credentials request
        let client_cred = OAuthTokenRequest::ClientCredentials(test_client_credentials_request());
        assert!(client_cred.is_client_credentials());
        assert!(client_cred.as_client_credentials().is_some());
        assert!(!client_cred.is_password());
    }

    #[test]
    fn test_serialization() {
        // Test authorization code serialization
        let auth_code = OAuthTokenRequest::AuthorizationCode(test_auth_code_request());
        let json = serde_json::to_string(&auth_code).unwrap();
        let parsed: OAuthTokenRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(auth_code, parsed);

        // Test refresh token serialization
        let refresh = OAuthTokenRequest::RefreshToken(test_refresh_token_request());
        let json = serde_json::to_string(&refresh).unwrap();
        let parsed: OAuthTokenRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(refresh, parsed);

        // Test password grant serialization
        let password = OAuthTokenRequest::Password(test_password_request());
        let json = serde_json::to_string(&password).unwrap();
        let parsed: OAuthTokenRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(password, parsed);

        // Test client credentials serialization
        let client_cred = OAuthTokenRequest::ClientCredentials(test_client_credentials_request());
        let json = serde_json::to_string(&client_cred).unwrap();
        let parsed: OAuthTokenRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(client_cred, parsed);
    }

    #[test]
    fn test_display() {
        let auth_code = OAuthTokenRequest::AuthorizationCode(test_auth_code_request());
        assert_eq!(auth_code.to_string(), "AuthorizationCodeGrant");

        let refresh = OAuthTokenRequest::RefreshToken(test_refresh_token_request());
        assert_eq!(refresh.to_string(), "RefreshTokenGrant");

        let password = OAuthTokenRequest::Password(test_password_request());
        assert_eq!(password.to_string(), "PasswordGrant");

        let client_cred = OAuthTokenRequest::ClientCredentials(test_client_credentials_request());
        assert_eq!(client_cred.to_string(), "ClientCredentialsGrant");
    }
}
