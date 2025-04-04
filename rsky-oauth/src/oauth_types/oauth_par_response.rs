use rocket::Responder;
use serde::{Deserialize, Serialize};

/// Response from a Pushed Authorization Request (PAR) endpoint.
///
/// As defined in RFC 9126 (OAuth 2.0 Pushed Authorization Requests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthParResponse {
    /// The request URI that the client will use in the subsequent authorization request
    request_uri: String,

    /// The expiration time of the request URI in seconds
    expires_in: u64,
}

impl OAuthParResponse {
    /// Create a new OAuthParResponse.
    ///
    /// # Errors
    /// Returns an error if the request URI is empty or if the expiration is zero.
    pub fn new(
        request_uri: impl Into<String>,
        expires_in: u64,
    ) -> Result<Self, OAuthParResponseError> {
        let request_uri = request_uri.into();
        if request_uri.is_empty() {
            return Err(OAuthParResponseError::EmptyRequestUri);
        }

        if expires_in == 0 {
            return Err(OAuthParResponseError::InvalidExpiration);
        }

        Ok(Self {
            request_uri,
            expires_in,
        })
    }

    /// Get the request URI.
    pub fn request_uri(&self) -> &str {
        &self.request_uri
    }

    /// Get the expiration time in seconds.
    pub fn expires_in(&self) -> u64 {
        self.expires_in
    }

    /// Parse from JSON.
    pub fn from_json(json: &str) -> Result<Self, OAuthParResponseError> {
        serde_json::from_str(json)
            .map_err(|_| OAuthParResponseError::InvalidJson)
            .and_then(|response: Self| {
                if response.request_uri.is_empty() {
                    return Err(OAuthParResponseError::EmptyRequestUri);
                }
                if response.expires_in == 0 {
                    return Err(OAuthParResponseError::InvalidExpiration);
                }
                Ok(response)
            })
    }

    /// Convert to JSON.
    pub fn to_json(&self) -> Result<String, OAuthParResponseError> {
        serde_json::to_string(self).map_err(|_| OAuthParResponseError::SerializationError)
    }
}

/// Errors that can occur when working with PAR responses.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OAuthParResponseError {
    #[error("Request URI cannot be empty")]
    EmptyRequestUri,

    #[error("Expiration must be a positive integer")]
    InvalidExpiration,

    #[error("Invalid JSON format")]
    InvalidJson,

    #[error("Error serializing to JSON")]
    SerializationError,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_par_response() {
        let response = OAuthParResponse::new("urn:example:request_uri", 60).unwrap();
        assert_eq!(response.request_uri(), "urn:example:request_uri");
        assert_eq!(response.expires_in(), 60);
    }

    #[test]
    fn test_invalid_par_response() {
        // Empty request URI
        assert!(matches!(
            OAuthParResponse::new("", 60),
            Err(OAuthParResponseError::EmptyRequestUri)
        ));

        // Zero expiration
        assert!(matches!(
            OAuthParResponse::new("urn:example:request_uri", 0),
            Err(OAuthParResponseError::InvalidExpiration)
        ));
    }

    #[test]
    fn test_from_json_valid() {
        let json = r#"{"request_uri":"urn:example:request_uri","expires_in":60}"#;
        let response = OAuthParResponse::from_json(json).unwrap();
        assert_eq!(response.request_uri(), "urn:example:request_uri");
        assert_eq!(response.expires_in(), 60);
    }

    #[test]
    fn test_from_json_invalid() {
        // Invalid JSON
        assert!(matches!(
            OAuthParResponse::from_json("not json"),
            Err(OAuthParResponseError::InvalidJson)
        ));

        // Empty request_uri
        assert!(matches!(
            OAuthParResponse::from_json(r#"{"request_uri":"","expires_in":60}"#),
            Err(OAuthParResponseError::EmptyRequestUri)
        ));

        // Zero expires_in
        assert!(matches!(
            OAuthParResponse::from_json(
                r#"{"request_uri":"urn:example:request_uri","expires_in":0}"#
            ),
            Err(OAuthParResponseError::InvalidExpiration)
        ));
    }

    #[test]
    fn test_serialize_deserialize() {
        let original = OAuthParResponse::new("urn:example:request_uri", 60).unwrap();
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: OAuthParResponse = serde_json::from_str(&serialized).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_to_json() {
        let response = OAuthParResponse::new("urn:example:request_uri", 60).unwrap();
        let json = response.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["request_uri"], "urn:example:request_uri");
        assert_eq!(parsed["expires_in"], 60);
    }

    #[test]
    fn test_roundtrip_json() {
        let original = OAuthParResponse::new("urn:example:request_uri", 60).unwrap();
        let json = original.to_json().unwrap();
        let parsed = OAuthParResponse::from_json(&json).unwrap();

        assert_eq!(original, parsed);
    }
}
