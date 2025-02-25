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

    // /// Convert the request to form-encoded parameters.
    // pub fn to_form_urlencoded(&self) -> String {
    //     match self {
    //         Self::AuthorizationCode(req) => {
    //             let mut params = vec![
    //                 ("grant_type".to_string(), "authorization_code".to_string()),
    //                 ("code".to_string(), req.code().to_string()),
    //                 ("redirect_uri".to_string(), req.redirect_uri().to_string()),
    //             ];
    //             if let Some(verifier) = req.code_verifier() {
    //                 params.push(("code_verifier".to_string(), verifier.to_string()));
    //             }
    //             form_urlencoded_params(&params)
    //         },
    //         Self::RefreshToken(req) => {
    //             let params = vec![
    //                 ("grant_type".to_string(), "refresh_token".to_string()),
    //                 ("refresh_token".to_string(), req.refresh_token().to_string()),
    //             ];
    //             form_urlencoded_params(&params)
    //         },
    //         Self::Password(req) => {
    //             let params = vec![
    //                 ("grant_type".to_string(), "password".to_string()),
    //                 ("username".to_string(), req.username().to_string()),
    //                 ("password".to_string(), req.password().to_string()),
    //             ];
    //             form_urlencoded_params(&params)
    //         },
    //         Self::ClientCredentials(_) => {
    //             form_urlencoded_params(&[("grant_type".to_string(), "client_credentials".to_string())])
    //         },
    //     }
    // }

    // /// Parse from form-encoded parameters.
    // pub fn from_form_urlencoded(form: &str) -> Result<Self, TokenRequestError> {
    //     let mut grant_type = None;
    //     let mut params = std::collections::HashMap::new();

    //     for pair in form.split('&') {
    //         let mut parts = pair.split('=');
    //         if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
    //             let key = urlencoding::decode(key)
    //                 .map_err(|_| TokenRequestError::InvalidFormEncoding)?;
    //             let value = urlencoding::decode(value)
    //                 .map_err(|_| TokenRequestError::InvalidFormEncoding)?;

    //             if key == "grant_type" {
    //                 grant_type = Some(value.into_owned());
    //             } else {
    //                 params.insert(key.into_owned(), value.into_owned());
    //             }
    //         }
    //     }

    //     let grant_type = grant_type.ok_or(TokenRequestError::MissingGrantType)?;

    //     match grant_type.as_str() {
    //         "authorization_code" => {
    //             // Convert params to authorization code request
    //             let code = params.get("code").ok_or(TokenRequestError::MissingParameter("code"))?;
    //             let redirect_uri = params.get("redirect_uri").ok_or(TokenRequestError::MissingParameter("redirect_uri"))?;
    //             let code_verifier = params.get("code_verifier").cloned();

    //             let request = OAuthAuthorizationCodeGrantTokenRequest::new(
    //                 code.clone(),
    //                 redirect_uri.clone(),
    //                 code_verifier,
    //             )?;
    //             Ok(Self::AuthorizationCode(request))
    //         },
    //         "refresh_token" => Ok(Self::RefreshToken(
    //             OAuthRefreshTokenGrantTokenRequest::from_form_urlencoded(form)?
    //         )),
    //         "password" => Ok(Self::Password(
    //             OAuthPasswordGrantTokenRequest::from_form_urlencoded(form)?
    //         )),
    //         "client_credentials" => Ok(Self::ClientCredentials(
    //             OAuthClientCredentialsGrantTokenRequest::new()
    //         )),
    //         _ => Err(TokenRequestError::InvalidGrantType(grant_type)),
    //     }
    // }
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
