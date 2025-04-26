//! OAuth Authorization Code Grant Token Request.
//!
//! Contains types and validation for authorization code grant token requests,
//! including PKCE code verifier validation.

use crate::oauth_provider::request::code::Code;
use crate::oauth_types::OAuthRedirectUri;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// The grant type for authorization code requests
const AUTHORIZATION_CODE_GRANT_TYPE: &str = "authorization_code";

/// An authorization code grant token request with PKCE support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthorizationCodeGrantTokenRequest {
    /// The authorization code received from the authorization server
    code: Code,

    /// The URI to redirect to after authorization
    redirect_uri: OAuthRedirectUri,

    /// Optional PKCE code verifier
    /// See RFC 7636 Section 4.1
    code_verifier: Option<String>,
}

// Custom serialization to flatten the structure
impl Serialize for OAuthAuthorizationCodeGrantTokenRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        use std::collections::HashMap;

        // Create a HashMap to store all entries
        // this might seem redundant but not doing causes a bug
        let mut entries = HashMap::new();

        // Add all fields to the HashMap
        entries.insert(
            "grant_type",
            serde_json::Value::String(AUTHORIZATION_CODE_GRANT_TYPE.to_string()),
        );
        entries.insert(
            "code",
            serde_json::Value::String(self.code.clone().into_inner()),
        );
        entries.insert(
            "redirect_uri",
            serde_json::Value::String(self.redirect_uri.as_str().to_string()),
        );

        // Add optional code_verifier if present
        if let Some(verifier) = &self.code_verifier {
            entries.insert("code_verifier", serde_json::Value::String(verifier.clone()));
        }

        // Serialize from the HashMap
        let mut map = serializer.serialize_map(Some(entries.len()))?;
        for (key, value) in entries {
            map.serialize_entry(key, &value)?;
        }

        map.end()
    }
}

// Custom deserialization to handle the flattened structure
impl<'de> Deserialize<'de> for OAuthAuthorizationCodeGrantTokenRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            grant_type: String,
            code: Code,
            redirect_uri: OAuthRedirectUri,
            #[serde(default)]
            code_verifier: Option<String>,
        }

        let helper = Helper::deserialize(deserializer)?;

        if helper.grant_type != AUTHORIZATION_CODE_GRANT_TYPE {
            return Err(serde::de::Error::custom(format!(
                "Invalid grant_type: expected '{}', got '{}'",
                AUTHORIZATION_CODE_GRANT_TYPE, helper.grant_type
            )));
        }

        Ok(OAuthAuthorizationCodeGrantTokenRequest {
            code: helper.code,
            redirect_uri: helper.redirect_uri,
            code_verifier: helper.code_verifier,
        })
    }
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
        code: Code,
        redirect_uri: OAuthRedirectUri,
        code_verifier: Option<impl Into<String>>,
    ) -> Result<Self, AuthorizationCodeGrantError> {
        let code_verifier = if let Some(verifier) = code_verifier {
            let verifier = verifier.into();
            validate_code_verifier(&verifier)?;
            Some(verifier)
        } else {
            None
        };

        Ok(Self {
            code,
            redirect_uri,
            code_verifier,
        })
    }

    /// Get the authorization code
    pub fn code(&self) -> &Code {
        &self.code
    }

    /// Get the redirect URI
    pub fn redirect_uri(&self) -> &OAuthRedirectUri {
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
            "AuthorizationCodeGrant(code={:?}, redirect_uri={}, has_verifier={})",
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

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     // Helper function to create a valid code verifier for tests
//     fn create_valid_code_verifier() -> String {
//         // Create a code verifier that meets the minimum length requirement of 43 characters
//         "1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJ-".to_string()
//     }
//
//     #[test]
//     fn test_new_valid_request() {
//         let request = OAuthAuthorizationCodeGrantTokenRequest::new(
//             Code::generate(),
//             OAuthRedirectUri::new("https://example.com/callback").unwrap(),
//             Some(create_valid_code_verifier()),
//         )
//         .unwrap();
//
//         assert_eq!(request.code().clone(), Code::new("valid_code").unwrap());
//         assert_eq!(
//             request.redirect_uri(),
//             &OAuthRedirectUri::new("https://example.com/callback").unwrap()
//         );
//         assert!(request.code_verifier().is_some());
//     }
//
//     #[test]
//     fn test_new_without_code_verifier() {
//         let request = OAuthAuthorizationCodeGrantTokenRequest::new(
//             Code::new("valid_code").unwrap(),
//             OAuthRedirectUri::new("https://example.com/callback").unwrap(),
//             None::<String>,
//         )
//         .unwrap();
//
//         assert_eq!(request.code().clone(), Code::new("valid_code").unwrap());
//         assert_eq!(
//             request.redirect_uri(),
//             &OAuthRedirectUri::new("https://example.com/callback").unwrap()
//         );
//         assert!(request.code_verifier().is_none());
//     }
//
//     #[test]
//     fn test_empty_code() {
//         let result = OAuthAuthorizationCodeGrantTokenRequest::new(
//             Code::new("valid_code").unwrap(),
//             OAuthRedirectUri::new("https://example.com/callback").unwrap(),
//             None::<String>,
//         );
//
//         assert_eq!(result.unwrap_err(), AuthorizationCodeGrantError::EmptyCode);
//     }
//
//     #[test]
//     fn test_invalid_code_verifier_length() {
//         // Too short
//         let result = OAuthAuthorizationCodeGrantTokenRequest::new(
//             Code::new("valid_code").unwrap(),
//             OAuthRedirectUri::new("https://example.com/callback").unwrap(),
//             Some("short"),
//         );
//         assert_eq!(
//             result.unwrap_err(),
//             AuthorizationCodeGrantError::InvalidCodeVerifierLength
//         );
//
//         // Too long
//         let result = OAuthAuthorizationCodeGrantTokenRequest::new(
//             Code::new("valid_code").unwrap(),
//             OAuthRedirectUri::new("https://example.com/callback").unwrap(),
//             Some("a".repeat(129)),
//         );
//         assert_eq!(
//             result.unwrap_err(),
//             AuthorizationCodeGrantError::InvalidCodeVerifierLength
//         );
//     }
//
//     #[test]
//     fn test_invalid_code_verifier_characters() {
//         let result = OAuthAuthorizationCodeGrantTokenRequest::new(
//             Code::new("valid_code").unwrap(),
//             OAuthRedirectUri::new("https://example.com/callback").unwrap(),
//             Some("1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJ!@#"),
//         );
//         assert_eq!(
//             result.unwrap_err(),
//             AuthorizationCodeGrantError::InvalidCodeVerifierCharacters
//         );
//     }
//
//     #[test]
//     fn test_serialize_deserialize() {
//         let request = OAuthAuthorizationCodeGrantTokenRequest::new(
//             Code::new("valid_code").unwrap(),
//             OAuthRedirectUri::new("https://example.com/callback").unwrap(),
//             Some(create_valid_code_verifier()),
//         )
//         .unwrap();
//
//         let serialized = serde_json::to_string(&request).unwrap();
//         println!("Serialized JSON: {}", serialized);
//
//         let deserialized: OAuthAuthorizationCodeGrantTokenRequest =
//             serde_json::from_str(&serialized).unwrap();
//
//         assert_eq!(request, deserialized);
//
//         // Verify the serialized structure
//         let json_value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
//         assert_eq!(json_value["grant_type"], "authorization_code");
//         assert_eq!(json_value["code"], "valid_code");
//         assert_eq!(json_value["redirect_uri"], "https://example.com/callback");
//         assert!(json_value["code_verifier"].is_string());
//     }
//
//     #[test]
//     fn test_display() {
//         let request = OAuthAuthorizationCodeGrantTokenRequest::new(
//             Code::new("valid_code").unwrap(),
//             OAuthRedirectUri::new("https://example.com/callback").unwrap(),
//             Some(create_valid_code_verifier()),
//         )
//         .unwrap();
//
//         assert_eq!(
//             request.to_string(),
//             "AuthorizationCodeGrant(code=valid_code, redirect_uri=https://example.com/callback, has_verifier=true)"
//         );
//     }
//
//     #[test]
//     fn test_deserialize_invalid_grant_type() {
//         let json = r#"
//         {
//             "grant_type": "invalid_type",
//             "code": "valid_code",
//             "redirect_uri": "https://example.com/callback"
//         }
//         "#;
//
//         let result = serde_json::from_str::<OAuthAuthorizationCodeGrantTokenRequest>(json);
//         assert!(result.is_err());
//     }
// }
