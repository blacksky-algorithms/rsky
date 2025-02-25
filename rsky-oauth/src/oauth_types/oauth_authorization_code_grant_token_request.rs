//! OAuth Authorization Code Grant Token Request.
//!
//! Contains types and validation for authorization code grant token requests,
//! including PKCE code verifier validation.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The grant type for authorization code requests
const AUTHORIZATION_CODE_GRANT_TYPE: &str = "authorization_code";

/// An authorization code grant token request with PKCE support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthAuthorizationCodeGrantTokenRequest {
    /// Must be "authorization_code"
    grant_type: String,

    /// The authorization code received from the authorization server
    code: String,

    /// The URI to redirect to after authorization
    redirect_uri: String,

    /// Optional PKCE code verifier
    /// See RFC 7636 Section 4.1
    #[serde(skip_serializing_if = "Option::is_none")]
    code_verifier: Option<String>,
}

impl OAuthAuthorizationCodeGrantTokenRequest {
    /// Create a new authorization code grant token request.
    ///
    /// # Arguments
    /// * `code` - The authorization code received from the authorization server
    /// * `redirect_uri` - The URI to redirect to after authorization
    /// * `code_verifier` - Optional PKCE code verifier (must be between 43-128 chars if provided)
    ///
    /// # Errors
    /// Returns an error if validation fails for any of the fields
    pub fn new(
        code: impl Into<String>,
        redirect_uri: impl Into<String>,
        code_verifier: Option<impl Into<String>>,
    ) -> Result<Self, AuthorizationCodeGrantError> {
        let code = code.into();
        if code.is_empty() {
            return Err(AuthorizationCodeGrantError::EmptyCode);
        }

        let redirect_uri = redirect_uri.into();
        if redirect_uri.is_empty() {
            return Err(AuthorizationCodeGrantError::EmptyRedirectUri);
        }

        let code_verifier = if let Some(verifier) = code_verifier {
            let verifier = verifier.into();
            validate_code_verifier(&verifier)?;
            Some(verifier)
        } else {
            None
        };

        Ok(Self {
            grant_type: AUTHORIZATION_CODE_GRANT_TYPE.to_string(),
            code,
            redirect_uri,
            code_verifier,
        })
    }

    /// Get the authorization code
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Get the redirect URI
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Get the code verifier if present
    pub fn code_verifier(&self) -> Option<&str> {
        self.code_verifier.as_deref()
    }
}

impl fmt::Display for OAuthAuthorizationCodeGrantTokenRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AuthorizationCodeGrant(code={}, redirect_uri={}, has_verifier={})",
            self.code,
            self.redirect_uri,
            self.code_verifier.is_some()
        )
    }
}

/// Validate a PKCE code verifier according to RFC 7636 Section 4.1
fn validate_code_verifier(verifier: &str) -> Result<(), AuthorizationCodeGrantError> {
    if verifier.len() < 43 || verifier.len() > 128 {
        return Err(AuthorizationCodeGrantError::InvalidCodeVerifierLength);
    }

    if !verifier.chars().all(|c| {
        matches!(c,
            'A'..='Z' |
            'a'..='z' |
            '0'..='9' |
            '-' | '.' | '_' | '~'
        )
    }) {
        return Err(AuthorizationCodeGrantError::InvalidCodeVerifierCharacters);
    }

    Ok(())
}

/// Errors that can occur when creating an authorization code grant token request
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthorizationCodeGrantError {
    #[error("Authorization code cannot be empty")]
    EmptyCode,

    #[error("Redirect URI cannot be empty")]
    EmptyRedirectUri,

    #[error("Code verifier must be between 43 and 128 characters")]
    InvalidCodeVerifierLength,

    #[error("Code verifier contains invalid characters")]
    InvalidCodeVerifierCharacters,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid_request() {
        let request = OAuthAuthorizationCodeGrantTokenRequest::new(
            "valid_code",
            "https://example.com/callback",
            Some("valid_code_verifier_thats_long_enough_to_meet_the_minimum_length_requirement"),
        )
        .unwrap();

        assert_eq!(request.code(), "valid_code");
        assert_eq!(request.redirect_uri(), "https://example.com/callback");
        assert!(request.code_verifier().is_some());
    }

    #[test]
    fn test_new_without_code_verifier() {
        let request = OAuthAuthorizationCodeGrantTokenRequest::new(
            "valid_code",
            "https://example.com/callback",
            None::<String>,
        )
        .unwrap();

        assert_eq!(request.code(), "valid_code");
        assert_eq!(request.redirect_uri(), "https://example.com/callback");
        assert!(request.code_verifier().is_none());
    }

    #[test]
    fn test_empty_code() {
        let result = OAuthAuthorizationCodeGrantTokenRequest::new(
            "",
            "https://example.com/callback",
            None::<String>,
        );

        assert_eq!(result.unwrap_err(), AuthorizationCodeGrantError::EmptyCode);
    }

    #[test]
    fn test_empty_redirect_uri() {
        let result = OAuthAuthorizationCodeGrantTokenRequest::new("valid_code", "", None::<String>);

        assert_eq!(
            result.unwrap_err(),
            AuthorizationCodeGrantError::EmptyRedirectUri
        );
    }

    #[test]
    fn test_invalid_code_verifier_length() {
        // Too short
        let result = OAuthAuthorizationCodeGrantTokenRequest::new(
            "valid_code",
            "https://example.com/callback",
            Some("short"),
        );
        assert_eq!(
            result.unwrap_err(),
            AuthorizationCodeGrantError::InvalidCodeVerifierLength
        );

        // Too long
        let result = OAuthAuthorizationCodeGrantTokenRequest::new(
            "valid_code",
            "https://example.com/callback",
            Some("a".repeat(129)),
        );
        assert_eq!(
            result.unwrap_err(),
            AuthorizationCodeGrantError::InvalidCodeVerifierLength
        );
    }

    #[test]
    fn test_invalid_code_verifier_characters() {
        let result = OAuthAuthorizationCodeGrantTokenRequest::new(
            "valid_code",
            "https://example.com/callback",
            Some("invalid!characters@in#this$long%enough^code&verifier*string"),
        );
        assert_eq!(
            result.unwrap_err(),
            AuthorizationCodeGrantError::InvalidCodeVerifierCharacters
        );
    }

    #[test]
    fn test_serialize_deserialize() {
        let request = OAuthAuthorizationCodeGrantTokenRequest::new(
            "valid_code",
            "https://example.com/callback",
            Some("valid_code_verifier_thats_long_enough_to_meet_the_minimum_length_requirement"),
        )
        .unwrap();

        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: OAuthAuthorizationCodeGrantTokenRequest =
            serde_json::from_str(&serialized).unwrap();

        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_display() {
        let request = OAuthAuthorizationCodeGrantTokenRequest::new(
            "valid_code",
            "https://example.com/callback",
            Some("valid_code_verifier_thats_long_enough_to_meet_the_minimum_length_requirement"),
        )
        .unwrap();

        assert_eq!(
            request.to_string(),
            "AuthorizationCodeGrant(code=valid_code, redirect_uri=https://example.com/callback, has_verifier=true)"
        );
    }
}
