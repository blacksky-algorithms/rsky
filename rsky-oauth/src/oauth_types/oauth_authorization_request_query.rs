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
    use super::*;

    // Helper function to create test parameters
    fn test_parameters() -> OAuthAuthorizationRequestParameters {
        unimplemented!("Need OAuthAuthorizationRequestParameters implementation")
    }

    // Helper function to create test JAR
    fn test_jar() -> OAuthAuthorizationRequestJar {
        unimplemented!("Need OAuthAuthorizationRequestJar implementation")
    }

    // Helper function to create test URI
    fn test_uri() -> OAuthAuthorizationRequestUri {
        unimplemented!("Need OAuthAuthorizationRequestUri implementation")
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
