use serde::{Deserialize, Serialize};

use crate::oauth_types::{OAuthClientId, CLIENT_ASSERTION_TYPE_JWT_BEARER};

/// Client credentials using JWT bearer token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientCredentialsJwtBearer {
    /// Client identifier
    pub client_id: OAuthClientId,

    /// Must be "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
    pub client_assertion_type: String,

    /// JWT with specific requirements:
    /// - "sub" must be the client_id of the OAuth client
    /// - "iat" is required and must be less than one minute
    /// - "aud" must contain value identifying the authorization server
    /// - "jti" (JWT ID) claim is optional but recommended
    pub client_assertion: String,
}

/// Client credentials using client secret (post method).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientCredentialsSecretPost {
    /// Client identifier
    pub client_id: OAuthClientId,

    /// Client secret
    pub client_secret: String,
}

/// Client credentials with no authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientCredentialsNone {
    /// Client identifier
    pub client_id: OAuthClientId,
}

/// All possible client credential types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OAuthClientCredentials {
    /// JWT bearer token credentials
    JwtBearer(OAuthClientCredentialsJwtBearer),
    /// Client secret credentials
    SecretPost(OAuthClientCredentialsSecretPost),
    /// No authentication credentials
    None(OAuthClientCredentialsNone),
}

impl OAuthClientCredentials {
    pub fn client_id(&self) -> OAuthClientId {
        match self {
            OAuthClientCredentials::JwtBearer(client) => client.client_id.clone(),
            OAuthClientCredentials::SecretPost(client) => client.client_id.clone(),
            OAuthClientCredentials::None(client) => client.client_id.clone(),
        }
    }
}

impl OAuthClientCredentialsJwtBearer {
    /// Create new JWT bearer client credentials.
    pub fn new(
        client_id: OAuthClientId,
        client_assertion: impl Into<String>,
    ) -> Result<Self, ClientCredentialsError> {
        let client_assertion = client_assertion.into();

        // Basic JWT format validation
        if !is_valid_jwt_format(&client_assertion) {
            return Err(ClientCredentialsError::InvalidJwtFormat);
        }

        Ok(Self {
            client_id,
            client_assertion_type: CLIENT_ASSERTION_TYPE_JWT_BEARER.to_string(),
            client_assertion,
        })
    }

    /// Validate the JWT assertion.
    ///
    /// This performs basic format validation. For full JWT validation,
    /// use proper JWT validation libraries.
    pub fn validate(&self) -> Result<(), ClientCredentialsError> {
        if !is_valid_jwt_format(&self.client_assertion) {
            return Err(ClientCredentialsError::InvalidJwtFormat);
        }

        if self.client_assertion_type != CLIENT_ASSERTION_TYPE_JWT_BEARER {
            return Err(ClientCredentialsError::InvalidAssertionType);
        }

        Ok(())
    }
}

impl OAuthClientCredentialsSecretPost {
    /// Create new client secret credentials.
    pub fn new(
        client_id: OAuthClientId,
        client_secret: impl Into<String>,
    ) -> Result<Self, ClientCredentialsError> {
        let client_secret = client_secret.into();
        if client_secret.is_empty() {
            return Err(ClientCredentialsError::EmptyClientSecret);
        }

        Ok(Self {
            client_id,
            client_secret,
        })
    }
}

impl OAuthClientCredentialsNone {
    /// Create new credentials with no authentication.
    pub fn new(client_id: OAuthClientId) -> Self {
        Self { client_id }
    }
}

impl OAuthClientCredentials {
    /// Get the client ID regardless of credential type.
    pub fn client_id(&self) -> &OAuthClientId {
        match self {
            Self::JwtBearer(creds) => &creds.client_id,
            Self::SecretPost(creds) => &creds.client_id,
            Self::None(creds) => &creds.client_id,
        }
    }
}

/// Basic validation of JWT format.
///
/// This only checks that the string contains two dots (three segments)
/// and no whitespace. For proper JWT validation, use a JWT library.
fn is_valid_jwt_format(jwt: &str) -> bool {
    if jwt.chars().any(char::is_whitespace) {
        return false;
    }
    jwt.chars().filter(|&c| c == '.').count() == 2
}

/// Errors that can occur with client credentials.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientCredentialsError {
    #[error("Invalid JWT format")]
    InvalidJwtFormat,

    #[error("Invalid client assertion type")]
    InvalidAssertionType,

    #[error("Client secret cannot be empty")]
    EmptyClientSecret,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a test client ID
    fn test_client_id() -> OAuthClientId {
        OAuthClientId::new("client123").unwrap()
    }

    #[test]
    fn test_jwt_bearer_creation() {
        let client_id = test_client_id();
        let jwt = "header.payload.signature";

        let creds =
            OAuthClientCredentialsJwtBearer::new(client_id.clone(), jwt.to_string()).unwrap();

        assert_eq!(creds.client_id, client_id);
        assert_eq!(
            creds.client_assertion_type,
            CLIENT_ASSERTION_TYPE_JWT_BEARER
        );
        assert_eq!(creds.client_assertion, jwt);
    }

    #[test]
    fn test_jwt_bearer_validation() {
        let client_id = test_client_id();

        // Invalid JWT format
        assert!(matches!(
            OAuthClientCredentialsJwtBearer::new(client_id.clone(), "invalid"),
            Err(ClientCredentialsError::InvalidJwtFormat)
        ));

        // JWT with whitespace
        assert!(matches!(
            OAuthClientCredentialsJwtBearer::new(client_id.clone(), "header. payload. signature"),
            Err(ClientCredentialsError::InvalidJwtFormat)
        ));

        // Valid JWT format
        let creds =
            OAuthClientCredentialsJwtBearer::new(client_id, "header.payload.signature").unwrap();
        assert!(creds.validate().is_ok());
    }

    #[test]
    fn test_secret_post_creation() {
        let client_id = test_client_id();

        // Empty secret
        assert!(matches!(
            OAuthClientCredentialsSecretPost::new(client_id.clone(), ""),
            Err(ClientCredentialsError::EmptyClientSecret)
        ));

        // Valid secret
        let creds = OAuthClientCredentialsSecretPost::new(client_id.clone(), "secret").unwrap();
        assert_eq!(creds.client_id, client_id);
        assert_eq!(creds.client_secret, "secret");
    }

    #[test]
    fn test_none_creation() {
        let client_id = test_client_id();
        let creds = OAuthClientCredentialsNone::new(client_id.clone());
        assert_eq!(creds.client_id, client_id);
    }

    #[test]
    fn test_enum_creation_and_access() {
        let client_id = test_client_id();

        // Create each variant
        let jwt_creds = OAuthClientCredentials::JwtBearer(
            OAuthClientCredentialsJwtBearer::new(client_id.clone(), "header.payload.signature")
                .unwrap(),
        );

        let secret_creds = OAuthClientCredentials::SecretPost(
            OAuthClientCredentialsSecretPost::new(client_id.clone(), "secret").unwrap(),
        );

        let none_creds =
            OAuthClientCredentials::None(OAuthClientCredentialsNone::new(client_id.clone()));

        // Test client_id access
        assert_eq!(jwt_creds.client_id(), &client_id);
        assert_eq!(secret_creds.client_id(), &client_id);
        assert_eq!(none_creds.client_id(), &client_id);
    }

    #[test]
    fn test_serialization() {
        let client_id = test_client_id();

        // JWT bearer credentials
        let jwt_creds =
            OAuthClientCredentialsJwtBearer::new(client_id.clone(), "header.payload.signature")
                .unwrap();

        let serialized = serde_json::to_string(&jwt_creds).unwrap();
        let deserialized: OAuthClientCredentialsJwtBearer =
            serde_json::from_str(&serialized).unwrap();
        assert_eq!(jwt_creds, deserialized);

        // Secret post credentials
        let secret_creds =
            OAuthClientCredentialsSecretPost::new(client_id.clone(), "secret").unwrap();

        let serialized = serde_json::to_string(&secret_creds).unwrap();
        let deserialized: OAuthClientCredentialsSecretPost =
            serde_json::from_str(&serialized).unwrap();
        assert_eq!(secret_creds, deserialized);

        // None credentials
        let none_creds = OAuthClientCredentialsNone::new(client_id);

        let serialized = serde_json::to_string(&none_creds).unwrap();
        let deserialized: OAuthClientCredentialsNone = serde_json::from_str(&serialized).unwrap();
        assert_eq!(none_creds, deserialized);
    }
}
