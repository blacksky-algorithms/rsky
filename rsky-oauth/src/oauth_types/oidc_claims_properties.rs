//! OIDC Claims Properties.
//!
//! These properties control how claims are requested in OpenID Connect.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Properties that can be specified for a requested claim in OpenID Connect.
///
/// These properties help specify additional requirements or constraints
/// on the requested claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcClaimsProperties {
    /// Whether this claim is essential for the request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub essential: Option<bool>,

    /// Expected value for the claim
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<ClaimsValue>,

    /// Set of acceptable values for the claim
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<ClaimsValue>>,
}

impl OidcClaimsProperties {
    /// Create a new empty OidcClaimsProperties
    pub fn new() -> Self {
        Self {
            essential: None,
            value: None,
            values: None,
        }
    }

    /// Set whether this claim is essential
    pub fn with_essential(mut self, essential: bool) -> Self {
        self.essential = Some(essential);
        self
    }

    /// Set the expected value for the claim
    pub fn with_value(mut self, value: impl Into<ClaimsValue>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set the acceptable values for the claim
    pub fn with_values(mut self, values: Vec<ClaimsValue>) -> Self {
        self.values = Some(values);
        self
    }

    /// Check if the properties object is empty (no properties set)
    pub fn is_empty(&self) -> bool {
        self.essential.is_none() && self.value.is_none() && self.values.is_none()
    }
}

impl Default for OidcClaimsProperties {
    fn default() -> Self {
        Self::new()
    }
}

/// Values that can be used in claim properties.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClaimsValue {
    /// String value
    String(String),
    /// Numeric value
    Number(i64),
    /// Boolean value
    Boolean(bool),
}

impl From<&str> for ClaimsValue {
    fn from(value: &str) -> Self {
        ClaimsValue::String(value.to_string())
    }
}

impl From<String> for ClaimsValue {
    fn from(value: String) -> Self {
        ClaimsValue::String(value)
    }
}

impl From<i64> for ClaimsValue {
    fn from(value: i64) -> Self {
        ClaimsValue::Number(value)
    }
}

impl From<i32> for ClaimsValue {
    fn from(value: i32) -> Self {
        ClaimsValue::Number(value as i64)
    }
}

impl From<u32> for ClaimsValue {
    fn from(value: u32) -> Self {
        ClaimsValue::Number(value as i64)
    }
}

impl From<bool> for ClaimsValue {
    fn from(value: bool) -> Self {
        ClaimsValue::Boolean(value)
    }
}

impl fmt::Display for ClaimsValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaimsValue::String(s) => write!(f, "{}", s),
            ClaimsValue::Number(n) => write!(f, "{}", n),
            ClaimsValue::Boolean(b) => write!(f, "{}", b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_empty_properties() {
        let properties = OidcClaimsProperties::new();
        assert!(properties.is_empty());

        let json = serde_json::to_value(&properties).unwrap();
        assert_eq!(json, json!({}));
    }

    #[test]
    fn test_with_essential() {
        let properties = OidcClaimsProperties::new().with_essential(true);

        assert!(!properties.is_empty());
        assert_eq!(properties.essential, Some(true));

        let json = serde_json::to_value(&properties).unwrap();
        assert_eq!(json, json!({"essential": true}));
    }

    #[test]
    fn test_with_value() {
        let string_value = OidcClaimsProperties::new().with_value("test_value");
        assert_eq!(
            string_value.value,
            Some(ClaimsValue::String("test_value".to_string()))
        );

        let number_value = OidcClaimsProperties::new().with_value(42i64);
        assert_eq!(number_value.value, Some(ClaimsValue::Number(42)));

        let bool_value = OidcClaimsProperties::new().with_value(true);
        assert_eq!(bool_value.value, Some(ClaimsValue::Boolean(true)));
    }

    #[test]
    fn test_with_values() {
        let properties = OidcClaimsProperties::new().with_values(vec![
            ClaimsValue::String("value1".to_string()),
            ClaimsValue::Number(42),
            ClaimsValue::Boolean(true),
        ]);

        assert!(!properties.is_empty());
        assert_eq!(
            properties.values,
            Some(vec![
                ClaimsValue::String("value1".to_string()),
                ClaimsValue::Number(42),
                ClaimsValue::Boolean(true)
            ])
        );

        let json = serde_json::to_value(&properties).unwrap();
        assert_eq!(json, json!({"values": ["value1", 42, true]}));
    }

    #[test]
    fn test_combined_properties() {
        let properties = OidcClaimsProperties::new()
            .with_essential(true)
            .with_value("preferred_value");

        let json = serde_json::to_value(&properties).unwrap();
        assert_eq!(
            json,
            json!({
                "essential": true,
                "value": "preferred_value"
            })
        );
    }

    #[test]
    fn test_claims_value_display() {
        assert_eq!(ClaimsValue::String("test".to_string()).to_string(), "test");
        assert_eq!(ClaimsValue::Number(42).to_string(), "42");
        assert_eq!(ClaimsValue::Boolean(true).to_string(), "true");
    }

    #[test]
    fn test_claims_value_conversions() {
        let string_literal: ClaimsValue = "test".into();
        assert_eq!(string_literal, ClaimsValue::String("test".to_string()));

        let string: ClaimsValue = "test".to_string().into();
        assert_eq!(string, ClaimsValue::String("test".to_string()));

        let i64_value: ClaimsValue = 42i64.into();
        assert_eq!(i64_value, ClaimsValue::Number(42));

        let i32_value: ClaimsValue = 42i32.into();
        assert_eq!(i32_value, ClaimsValue::Number(42));

        let u32_value: ClaimsValue = 42u32.into();
        assert_eq!(u32_value, ClaimsValue::Number(42));

        let bool_value: ClaimsValue = true.into();
        assert_eq!(bool_value, ClaimsValue::Boolean(true));
    }

    #[test]
    fn test_serialize_deserialize() {
        let original = OidcClaimsProperties::new()
            .with_essential(true)
            .with_value("test_value");

        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: OidcClaimsProperties = serde_json::from_str(&serialized).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_deserialize_from_json() {
        let json = r#"{"essential":true,"value":"test_value","values":[42,true,"str"]}"#;
        let properties: OidcClaimsProperties = serde_json::from_str(json).unwrap();

        assert_eq!(properties.essential, Some(true));
        assert_eq!(
            properties.value,
            Some(ClaimsValue::String("test_value".to_string()))
        );
        assert_eq!(
            properties.values,
            Some(vec![
                ClaimsValue::Number(42),
                ClaimsValue::Boolean(true),
                ClaimsValue::String("str".to_string())
            ])
        );
    }
}
