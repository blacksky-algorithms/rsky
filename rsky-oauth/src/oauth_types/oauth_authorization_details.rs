use serde::{Deserialize, Serialize};
use url::Url;

/// A single authorization detail object as defined in RFC 9396, Section 2.
///
/// An authorization detail object provides a way for the client to specify
/// the details of the authorization being requested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthAuthorizationDetail {
    /// The type identifier for the authorization detail
    #[serde(rename = "type")]
    type_: String,

    /// An array of strings representing the location of the resource or RS
    #[serde(skip_serializing_if = "Option::is_none")]
    locations: Option<Vec<String>>,

    /// An array of strings representing the actions to be taken at the resource
    #[serde(skip_serializing_if = "Option::is_none")]
    actions: Option<Vec<String>>,

    /// An array of strings representing the data types being requested
    #[serde(skip_serializing_if = "Option::is_none")]
    datatypes: Option<Vec<String>>,

    /// A string identifier indicating a specific resource available at the API
    #[serde(skip_serializing_if = "Option::is_none")]
    identifier: Option<String>,

    /// An array of strings representing privileges being requested
    #[serde(skip_serializing_if = "Option::is_none")]
    privileges: Option<Vec<String>>,

    /// Additional fields that aren't part of the core specification
    #[serde(flatten)]
    additional_fields: std::collections::HashMap<String, serde_json::Value>,
}

impl OAuthAuthorizationDetail {
    /// Create a new OAuthAuthorizationDetail with required type.
    pub fn new(type_: impl Into<String>) -> Self {
        Self {
            type_: type_.into(),
            locations: None,
            actions: None,
            datatypes: None,
            identifier: None,
            privileges: None,
            additional_fields: std::collections::HashMap::new(),
        }
    }

    /// Get the type identifier.
    pub fn type_(&self) -> &str {
        &self.type_
    }

    /// Set the locations.
    pub fn with_locations(
        mut self,
        locations: Vec<String>,
    ) -> Result<Self, AuthorizationDetailError> {
        // Validate URLs
        for location in &locations {
            Url::parse(location).map_err(|_| AuthorizationDetailError::InvalidLocationUrl)?;
        }
        self.locations = Some(locations);
        Ok(self)
    }

    /// Get the locations, if any.
    pub fn locations(&self) -> Option<&[String]> {
        self.locations.as_deref()
    }

    /// Set the actions.
    pub fn with_actions(mut self, actions: Vec<String>) -> Self {
        self.actions = Some(actions);
        self
    }

    /// Get the actions, if any.
    pub fn actions(&self) -> Option<&[String]> {
        self.actions.as_deref()
    }

    /// Set the datatypes.
    pub fn with_datatypes(mut self, datatypes: Vec<String>) -> Self {
        self.datatypes = Some(datatypes);
        self
    }

    /// Get the datatypes, if any.
    pub fn datatypes(&self) -> Option<&[String]> {
        self.datatypes.as_deref()
    }

    /// Set the identifier.
    pub fn with_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.identifier = Some(identifier.into());
        self
    }

    /// Get the identifier, if any.
    pub fn identifier(&self) -> Option<&str> {
        self.identifier.as_deref()
    }

    /// Set the privileges.
    pub fn with_privileges(mut self, privileges: Vec<String>) -> Self {
        self.privileges = Some(privileges);
        self
    }

    /// Get the privileges, if any.
    pub fn privileges(&self) -> Option<&[String]> {
        self.privileges.as_deref()
    }

    /// Add an additional field.
    pub fn with_additional_field(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.additional_fields.insert(key.into(), value.into());
        self
    }

    /// Parse from JSON.
    pub fn from_json(json: &str) -> Result<Self, AuthorizationDetailError> {
        serde_json::from_str(json).map_err(|_| AuthorizationDetailError::InvalidJson)
    }

    /// Convert to JSON.
    pub fn to_json(&self) -> Result<String, AuthorizationDetailError> {
        serde_json::to_string(self).map_err(|_| AuthorizationDetailError::SerializationError)
    }
}

/// A collection of authorization details as defined in RFC 9396, Section 2.
pub type OAuthAuthorizationDetails = Vec<OAuthAuthorizationDetail>;

/// Errors that can occur when working with authorization details.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthorizationDetailError {
    #[error("Invalid location URL")]
    InvalidLocationUrl,

    #[error("Invalid JSON format")]
    InvalidJson,

    #[error("Error serializing to JSON")]
    SerializationError,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_basic_detail() {
        let detail = OAuthAuthorizationDetail::new("payment_initiation");
        assert_eq!(detail.type_(), "payment_initiation");
        assert!(detail.locations().is_none());
        assert!(detail.actions().is_none());
        assert!(detail.datatypes().is_none());
        assert!(detail.identifier().is_none());
        assert!(detail.privileges().is_none());
    }

    #[test]
    fn test_with_locations() {
        let detail = OAuthAuthorizationDetail::new("account_information")
            .with_locations(vec![
                "https://example.com/accounts".to_string(),
                "https://example.org/banking".to_string(),
            ])
            .unwrap();

        assert_eq!(
            detail.locations(),
            Some(
                &[
                    "https://example.com/accounts".to_string(),
                    "https://example.org/banking".to_string(),
                ][..]
            )
        );
    }

    #[test]
    fn test_invalid_location() {
        let result = OAuthAuthorizationDetail::new("account_information")
            .with_locations(vec!["not a url".to_string()]);

        assert!(matches!(
            result,
            Err(AuthorizationDetailError::InvalidLocationUrl)
        ));
    }

    #[test]
    fn test_with_actions() {
        let detail = OAuthAuthorizationDetail::new("account_information")
            .with_actions(vec!["read".to_string(), "write".to_string()]);

        assert_eq!(
            detail.actions(),
            Some(&["read".to_string(), "write".to_string()][..])
        );
    }

    #[test]
    fn test_with_datatypes() {
        let detail = OAuthAuthorizationDetail::new("account_information")
            .with_datatypes(vec!["balance".to_string(), "transactions".to_string()]);

        assert_eq!(
            detail.datatypes(),
            Some(&["balance".to_string(), "transactions".to_string()][..])
        );
    }

    #[test]
    fn test_with_identifier() {
        let detail =
            OAuthAuthorizationDetail::new("account_information").with_identifier("account123");

        assert_eq!(detail.identifier(), Some("account123"));
    }

    #[test]
    fn test_with_privileges() {
        let detail = OAuthAuthorizationDetail::new("account_information")
            .with_privileges(vec!["admin".to_string(), "user".to_string()]);

        assert_eq!(
            detail.privileges(),
            Some(&["admin".to_string(), "user".to_string()][..])
        );
    }

    #[test]
    fn test_with_additional_field() {
        let detail = OAuthAuthorizationDetail::new("account_information")
            .with_additional_field("custom_field", "custom_value");

        // We need to check serialization to verify the additional field
        let json = detail.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["custom_field"], "custom_value");
    }

    #[test]
    fn test_serialize_deserialize() {
        let original = OAuthAuthorizationDetail::new("account_information")
            .with_locations(vec!["https://example.com/accounts".to_string()])
            .unwrap()
            .with_actions(vec!["read".to_string()])
            .with_identifier("account123");

        let json = original.to_json().unwrap();
        let deserialized = OAuthAuthorizationDetail::from_json(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_from_json() {
        let json = r#"{
            "type": "account_information",
            "locations": ["https://example.com/accounts"],
            "actions": ["read", "write"],
            "identifier": "account123",
            "custom_field": "custom_value"
        }"#;

        let detail = OAuthAuthorizationDetail::from_json(json).unwrap();

        assert_eq!(detail.type_(), "account_information");
        assert_eq!(
            detail.locations(),
            Some(&["https://example.com/accounts".to_string()][..])
        );
        assert_eq!(
            detail.actions(),
            Some(&["read".to_string(), "write".to_string()][..])
        );
        assert_eq!(detail.identifier(), Some("account123"));

        let json = detail.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["custom_field"], "custom_value");
    }

    #[test]
    fn test_authorization_details_collection() {
        let details = vec![
            OAuthAuthorizationDetail::new("account_information"),
            OAuthAuthorizationDetail::new("payment_initiation"),
        ];

        assert_eq!(details.len(), 2);
        assert_eq!(details[0].type_(), "account_information");
        assert_eq!(details[1].type_(), "payment_initiation");

        // Test JSON serialization of the collection
        let json = serde_json::to_string(&details).unwrap();
        let deserialized: OAuthAuthorizationDetails = serde_json::from_str(&json).unwrap();

        assert_eq!(details, deserialized);
    }
}
