use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Client Credentials Grant token request.
///
/// This represents a request to obtain an access token using
/// the client credentials grant type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthClientCredentialsGrantTokenRequest {
    /// Must be "client_credentials"
    grant_type: GrantType,
}

/// Grant type for client credentials flow
/// or for refresh token requests
/// or Grant type for password credentials flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    /// Client credentials grant type
    ClientCredentials,
    /// Refresh token grant type
    RefreshToken,
    /// Password grant type
    Password,
}

// Custom serialization to output the correct JSON structure
impl Serialize for OAuthClientCredentialsGrantTokenRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("grant_type", "client_credentials")?;
        map.end()
    }
}

// Custom deserialization to verify the grant_type value
impl<'de> Deserialize<'de> for OAuthClientCredentialsGrantTokenRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            grant_type: String,
        }

        let helper = Helper::deserialize(deserializer)?;

        if helper.grant_type != "client_credentials" {
            return Err(serde::de::Error::custom(format!(
                "Invalid grant_type: expected 'client_credentials', got '{}'",
                helper.grant_type
            )));
        }

        Ok(OAuthClientCredentialsGrantTokenRequest::new())
    }
}

impl OAuthClientCredentialsGrantTokenRequest {
    /// Create a new client credentials grant token request.
    pub fn new() -> Self {
        Self {
            grant_type: GrantType::ClientCredentials,
        }
    }
}

impl Default for OAuthClientCredentialsGrantTokenRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for OAuthClientCredentialsGrantTokenRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ClientCredentialsGrant")
    }
}

impl fmt::Display for GrantType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientCredentials => write!(f, "client_credentials"),
            Self::Password => write!(f, "password"),
            Self::RefreshToken => write!(f, "refresh_token"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let request = OAuthClientCredentialsGrantTokenRequest::new();
        // No fields to check
        assert_eq!(
            request,
            OAuthClientCredentialsGrantTokenRequest {
                grant_type: GrantType::ClientCredentials
            }
        );
    }

    #[test]
    fn test_default() {
        let request = OAuthClientCredentialsGrantTokenRequest::default();
        assert_eq!(request, OAuthClientCredentialsGrantTokenRequest::new());
    }

    #[test]
    fn test_display() {
        let request = OAuthClientCredentialsGrantTokenRequest::new();
        assert_eq!(request.to_string(), "ClientCredentialsGrant");
        assert_eq!(
            GrantType::ClientCredentials.to_string(),
            "client_credentials"
        );
    }

    #[test]
    fn test_serialization() {
        let request = OAuthClientCredentialsGrantTokenRequest::new();

        let serialized = serde_json::to_string(&request).unwrap();
        println!("Serialized JSON: {}", serialized);

        let expected = r#"{"grant_type":"client_credentials"}"#;
        assert_eq!(serialized, expected);

        let deserialized: OAuthClientCredentialsGrantTokenRequest =
            serde_json::from_str(&serialized).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_deserialize_invalid_grant_type() {
        let json = r#"{"grant_type":"invalid_type"}"#;
        let result = serde_json::from_str::<OAuthClientCredentialsGrantTokenRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_serialization_case_handling() {
        // Test that we can deserialize with different case variants
        let json = r#"{"grant_type":"CLIENT_CREDENTIALS"}"#;
        let result = serde_json::from_str::<OAuthClientCredentialsGrantTokenRequest>(json);
        assert!(result.is_err(), "Should not accept uppercase");

        let json = r#"{"grant_type":"client_credentials"}"#;
        let result = serde_json::from_str::<OAuthClientCredentialsGrantTokenRequest>(json);
        assert!(result.is_ok(), "Should accept snake_case");
    }
}
