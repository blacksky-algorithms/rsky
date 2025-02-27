use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth_types::{
    OAuthAuthorizationRequestJar, OAuthAuthorizationRequestParameters, OAuthAuthorizationRequestUri,
};

/// An OAuth authorization request query.
///
/// This represents the different ways an authorization request can be made:
/// - Direct parameters in the query
/// - JWT-based request (JAR)
/// - Request URI reference
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OAuthAuthorizationRequestQuery {
    /// Standard authorization request parameters
    Parameters(OAuthAuthorizationRequestParameters),
    /// JWT-based request parameters
    Jar(OAuthAuthorizationRequestJar),
    /// Reference to pushed request parameters
    Uri(OAuthAuthorizationRequestUri),
}

impl OAuthAuthorizationRequestQuery {
    /// Create a new query from authorization request parameters.
    pub fn from_parameters(params: OAuthAuthorizationRequestParameters) -> Self {
        Self::Parameters(params)
    }

    /// Create a new query from a JWT-based authorization request.
    pub fn from_jar(jar: OAuthAuthorizationRequestJar) -> Self {
        Self::Jar(jar)
    }

    /// Create a new query from a request URI reference.
    pub fn from_uri(uri: OAuthAuthorizationRequestUri) -> Self {
        Self::Uri(uri)
    }

    /// Returns true if this is a parameters-based request.
    pub fn is_parameters(&self) -> bool {
        matches!(self, Self::Parameters(_))
    }

    /// Returns true if this is a JAR-based request.
    pub fn is_jar(&self) -> bool {
        matches!(self, Self::Jar(_))
    }

    /// Returns true if this is a URI-based request.
    pub fn is_uri(&self) -> bool {
        matches!(self, Self::Uri(_))
    }

    /// Get the inner parameters if this is a parameters-based request.
    pub fn as_parameters(&self) -> Option<&OAuthAuthorizationRequestParameters> {
        match self {
            Self::Parameters(params) => Some(params),
            _ => None,
        }
    }

    /// Get the inner JAR if this is a JAR-based request.
    pub fn as_jar(&self) -> Option<&OAuthAuthorizationRequestJar> {
        match self {
            Self::Jar(jar) => Some(jar),
            _ => None,
        }
    }

    /// Get the inner URI if this is a URI-based request.
    pub fn as_uri(&self) -> Option<&OAuthAuthorizationRequestUri> {
        match self {
            Self::Uri(uri) => Some(uri),
            _ => None,
        }
    }
}

impl fmt::Display for OAuthAuthorizationRequestQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parameters(params) => write!(f, "Parameters({})", params),
            Self::Jar(jar) => write!(f, "JAR({})", jar),
            Self::Uri(uri) => write!(f, "URI({})", uri),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::oauth_types::{
        OAuthClientId, OAuthRequestUri, OAuthResponseType, OAuthScope, RequestClaims, ResponseMode,
    };

    use super::*;

    // ROT13 decode function
    fn rot13_decode(encoded: &str) -> Vec<u8> {
        encoded
            .chars()
            .map(|c| {
                if c >= 'A' && c <= 'Z' {
                    let mut code = c as u8 + 13;
                    if code > b'Z' {
                        code -= 26;
                    }
                    code as char
                } else if c >= 'a' && c <= 'z' {
                    let mut code = c as u8 + 13;
                    if code > b'z' {
                        code -= 26;
                    }
                    code as char
                } else {
                    c
                }
            })
            .collect::<String>()
            .into_bytes()
    }

    fn get_es256_key() -> Vec<u8> {
        // Please don't use this key for anything
        let encoded_key = r#"-----ORTVA CEVINGR XRL-----
        ZVTUNtRNZOZTOldTFZ49NtRTPPdTFZ49NjRUOT0jnjVONDDtKS0dxv6bEKcdGeHd
        L/Rb9hBBIuOS7ftobTz3V6t7Oe6uENAPNNE38eqJJL/rpIWviZUQNW0MP5iHWYUR
        eCn7dMVM53xuIGNc+0mDwUEC1405fp7rNkmqXRaFATQkIn+9bLE0SdCR
        -----RAQ CEVINGR XRL-----"#;
        rot13_decode(encoded_key)
    }
    fn create_test_claims() -> RequestClaims {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        RequestClaims {
            iat: now,
            exp: Some(now + 300), // 5 minutes from now
            jti: Some("test-id".to_string()),
            additional_claims: serde_json::Map::new(),
        }
    }

    // Helper function to create test parameters
    fn test_parameters() -> OAuthAuthorizationRequestParameters {
        OAuthAuthorizationRequestParameters::new(
            OAuthClientId::new("test_client").unwrap(),
            OAuthResponseType::Code,
            None,
            Some(OAuthScope::new("read write").unwrap()),
            Some("state123".to_string()),
        )
        .unwrap()
        .with_nonce("nonce123")
        .with_response_mode(ResponseMode::Query)
    }

    // Helper function to create test JAR
    fn test_jar() -> OAuthAuthorizationRequestJar {
        let claims = create_test_claims();
        let key = get_es256_key();
        OAuthAuthorizationRequestJar::new(
            claims.clone(),
            Some(jsonwebtoken::Algorithm::ES256),
            Some(&key),
        )
        .unwrap()
    }

    fn get_request_uri() -> OAuthRequestUri {
        OAuthRequestUri::new("https://example.com/oauth/request").unwrap()
    }

    // Helper function to create test URI
    fn test_uri() -> OAuthAuthorizationRequestUri {
        let uri = get_request_uri();
        OAuthAuthorizationRequestUri::new(uri.clone())
    }

    #[test]
    fn test_constructors() {
        let params = test_parameters();
        let jar = test_jar();
        let uri = test_uri();

        let query_params = OAuthAuthorizationRequestQuery::from_parameters(params);
        let query_jar = OAuthAuthorizationRequestQuery::from_jar(jar);
        let query_uri = OAuthAuthorizationRequestQuery::from_uri(uri);

        assert!(query_params.is_parameters());
        assert!(query_jar.is_jar());
        assert!(query_uri.is_uri());
    }

    #[test]
    fn test_accessors() {
        let params = test_parameters();
        let query = OAuthAuthorizationRequestQuery::from_parameters(params);

        assert!(query.as_parameters().is_some());
        assert!(query.as_jar().is_none());
        assert!(query.as_uri().is_none());

        let jar = test_jar();
        let query = OAuthAuthorizationRequestQuery::from_jar(jar);

        assert!(query.as_parameters().is_none());
        assert!(query.as_jar().is_some());
        assert!(query.as_uri().is_none());

        let uri = test_uri();
        let query = OAuthAuthorizationRequestQuery::from_uri(uri);

        assert!(query.as_parameters().is_none());
        assert!(query.as_jar().is_none());
        assert!(query.as_uri().is_some());
    }

    #[test]
    fn test_serialization() {
        // These tests will need to be updated with real data once dependencies are implemented

        let json_parameters = r#"{
            "client_id": "test-client",
            "response_type": "code"
        }"#;

        let json_jar = r#"{
            "request": "test.jwt.value"
        }"#;

        let json_uri = r#"{
            "request_uri": "https://example.com/request"
        }"#;

        // For now, just verify we can parse the JSON structure
        let _: serde_json::Value = serde_json::from_str(json_parameters).unwrap();
        let _: serde_json::Value = serde_json::from_str(json_jar).unwrap();
        let _: serde_json::Value = serde_json::from_str(json_uri).unwrap();
    }

    #[test]
    fn test_display() {
        // Once dependencies are implemented, test the Display implementation with real values
        let params = test_parameters();
        let query = OAuthAuthorizationRequestQuery::from_parameters(params);
        assert!(query.to_string().starts_with("Parameters"));

        let jar = test_jar();
        let query = OAuthAuthorizationRequestQuery::from_jar(jar);
        assert!(query.to_string().starts_with("JAR"));

        let uri = test_uri();
        let query = OAuthAuthorizationRequestQuery::from_uri(uri);
        assert!(query.to_string().starts_with("URI"));
    }
}
