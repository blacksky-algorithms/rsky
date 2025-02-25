use crate::oauth_types::GrantType;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Password grant token request.
///
/// Represents a request to obtain an access token using the
/// resource owner password credentials grant type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthPasswordGrantTokenRequest {
    /// Must be "password"
    grant_type: GrantType,

    /// The resource owner's username
    username: String,

    /// The resource owner's password
    password: String,
}

impl OAuthPasswordGrantTokenRequest {
    /// Create a new password grant token request.
    ///
    /// # Arguments
    /// * `username` - The resource owner's username
    /// * `password` - The resource owner's password
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self, PasswordGrantError> {
        let username = username.into();
        let password = password.into();

        if username.is_empty() {
            return Err(PasswordGrantError::EmptyUsername);
        }

        if password.is_empty() {
            return Err(PasswordGrantError::EmptyPassword);
        }

        Ok(Self {
            grant_type: GrantType::Password,
            username,
            password,
        })
    }

    /// Get the username
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Get the password
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Display for OAuthPasswordGrantTokenRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PasswordGrant(username={})", self.username)
    }
}

/// Errors that can occur when creating a password grant token request.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PasswordGrantError {
    #[error("Username cannot be empty")]
    EmptyUsername,

    #[error("Password cannot be empty")]
    EmptyPassword,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid_request() {
        let request = OAuthPasswordGrantTokenRequest::new("user", "pass").unwrap();
        assert_eq!(request.username(), "user");
        assert_eq!(request.password(), "pass");
    }

    #[test]
    fn test_new_empty_username() {
        assert!(matches!(
            OAuthPasswordGrantTokenRequest::new("", "pass"),
            Err(PasswordGrantError::EmptyUsername)
        ));
    }

    #[test]
    fn test_new_empty_password() {
        assert!(matches!(
            OAuthPasswordGrantTokenRequest::new("user", ""),
            Err(PasswordGrantError::EmptyPassword)
        ));
    }

    #[test]
    fn test_display() {
        let request = OAuthPasswordGrantTokenRequest::new("user", "pass").unwrap();
        assert_eq!(request.to_string(), "PasswordGrant(username=user)");
    }

    #[test]
    fn test_serialization() {
        let request = OAuthPasswordGrantTokenRequest::new("user", "pass").unwrap();

        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: OAuthPasswordGrantTokenRequest =
            serde_json::from_str(&serialized).unwrap();

        assert_eq!(request, deserialized);

        // Check JSON structure
        let json_value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(json_value["grant_type"], "password");
        assert_eq!(json_value["username"], "user");
        assert_eq!(json_value["password"], "pass");
    }
}
