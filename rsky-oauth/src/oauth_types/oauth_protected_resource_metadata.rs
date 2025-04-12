use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth_types::{OAuthIssuerIdentifier, WebUri};

/// OAuth Protected Resource Metadata.
///
/// This metadata describes a protected resource that clients can interact with
/// and identifies which authorization servers can be used to obtain access tokens.
/// See draft-ietf-oauth-resource-metadata-05.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthProtectedResourceMetadata {
    /// The protected resource's resource identifier
    pub resource: WebUri,

    /// Authorization servers that can be used with this protected resource
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_servers: Option<Vec<OAuthIssuerIdentifier>>,

    /// URL of the protected resource's JWK Set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<WebUri>,

    /// Supported scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,

    /// Supported bearer token presentation methods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_methods_supported: Option<Vec<BearerMethod>>,

    /// Supported signing algorithms for resource responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_signing_alg_values_supported: Option<Vec<String>>,

    /// URL to documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_documentation: Option<WebUri>,

    /// URL to resource policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_policy_uri: Option<WebUri>,

    /// URL to terms of service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_tos_uri: Option<WebUri>,
}

/// Bearer token presentation methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BearerMethod {
    /// Present token in Authorization header
    Header,
    /// Present token in request body
    Body,
    /// Present token in query string
    Query,
}

impl fmt::Display for BearerMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BearerMethod::Header => write!(f, "header"),
            BearerMethod::Body => write!(f, "body"),
            BearerMethod::Query => write!(f, "query"),
        }
    }
}

impl OAuthProtectedResourceMetadata {
    /// Create new metadata for a protected resource.
    pub fn new(resource: WebUri) -> Self {
        Self {
            resource,
            authorization_servers: None,
            jwks_uri: None,
            scopes_supported: None,
            bearer_methods_supported: None,
            resource_signing_alg_values_supported: None,
            resource_documentation: None,
            resource_policy_uri: None,
            resource_tos_uri: None,
        }
    }

    /// Add authorization servers that can be used with this resource.
    pub fn with_authorization_servers(mut self, servers: Vec<OAuthIssuerIdentifier>) -> Self {
        self.authorization_servers = Some(servers);
        self
    }

    /// Set the URL for the resource's JWK Set.
    pub fn with_jwks_uri(mut self, uri: WebUri) -> Self {
        self.jwks_uri = Some(uri);
        self
    }

    /// Set the supported scopes.
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes_supported = Some(scopes);
        self
    }

    /// Set the supported bearer token methods.
    pub fn with_bearer_methods(mut self, methods: Vec<BearerMethod>) -> Self {
        self.bearer_methods_supported = Some(methods);
        self
    }

    /// Set the supported signing algorithms.
    pub fn with_signing_algorithms(mut self, algorithms: Vec<String>) -> Self {
        self.resource_signing_alg_values_supported = Some(algorithms);
        self
    }

    /// Set the documentation URL.
    pub fn with_documentation(mut self, uri: WebUri) -> Self {
        self.resource_documentation = Some(uri);
        self
    }

    /// Set the policy URL.
    pub fn with_policy(mut self, uri: WebUri) -> Self {
        self.resource_policy_uri = Some(uri);
        self
    }

    /// Set the terms of service URL.
    pub fn with_tos(mut self, uri: WebUri) -> Self {
        self.resource_tos_uri = Some(uri);
        self
    }

    /// Convert the metadata to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Parse metadata from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl fmt::Display for OAuthProtectedResourceMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Protected Resource: {}", self.resource)
    }
}

#[cfg(test)]
mod tests {
    use crate::oauth_types::ValidUri;

    use super::*;

    // Helper function to create a test WebUri
    fn test_uri() -> WebUri {
        WebUri::validate("https://example.com").unwrap()
    }

    // Helper function to create a test IssuerIdentifier
    fn test_issuer() -> OAuthIssuerIdentifier {
        OAuthIssuerIdentifier::new("https://example.com").unwrap()
    }

    #[test]
    fn test_bearer_method_display() {
        assert_eq!(BearerMethod::Header.to_string(), "header");
        assert_eq!(BearerMethod::Body.to_string(), "body");
        assert_eq!(BearerMethod::Query.to_string(), "query");
    }

    #[test]
    fn test_bearer_method_serialization() {
        let json = serde_json::to_string(&BearerMethod::Header).unwrap();
        assert_eq!(json, "\"header\"");

        let deserialized: BearerMethod = serde_json::from_str("\"header\"").unwrap();
        assert_eq!(deserialized, BearerMethod::Header);
    }

    #[test]
    fn test_metadata_builder() {
        let resource = test_uri();
        let metadata = OAuthProtectedResourceMetadata::new(resource.clone())
            .with_authorization_servers(vec![test_issuer()])
            .with_bearer_methods(vec![BearerMethod::Header, BearerMethod::Body])
            .with_scopes(vec!["read".to_string(), "write".to_string()]);

        assert_eq!(metadata.resource, resource);
        assert!(metadata.authorization_servers.is_some());
        assert_eq!(metadata.bearer_methods_supported.as_ref().unwrap().len(), 2);
        assert_eq!(metadata.scopes_supported.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_metadata_serialization() {
        let resource = test_uri();
        let metadata = OAuthProtectedResourceMetadata::new(resource)
            .with_bearer_methods(vec![BearerMethod::Header]);

        let json = metadata.to_json().unwrap();
        let deserialized = OAuthProtectedResourceMetadata::from_json(&json).unwrap();

        assert_eq!(metadata, deserialized);
    }

    #[test]
    fn test_display() {
        let resource = test_uri();
        let metadata = OAuthProtectedResourceMetadata::new(resource);
        assert!(metadata.to_string().starts_with("Protected Resource:"));
    }
}
