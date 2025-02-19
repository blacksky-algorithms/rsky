use std::fmt;

/// A validated OAuth access token.
/// 
/// This is a newtype wrapper around String that ensures the token is not empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAccessToken(String);

impl OAuthAccessToken {
    /// Create a new OAuthAccessToken.
    ///
    /// # Errors
    /// Returns an error if the token string is empty.
    pub fn new(token: impl Into<String>) -> Result<Self, AccessTokenError> {
        let token = token.into();
        if token.is_empty() {
            return Err(AccessTokenError::Empty);
        }
        Ok(Self(token))
    }

    /// Get the underlying token string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for OAuthAccessToken {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OAuthAccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Errors that can occur when creating an OAuthAccessToken.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AccessTokenError {
    #[error("Access token cannot be empty")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid_token() {
        let token = OAuthAccessToken::new("valid_token").unwrap();
        assert_eq!(token.as_ref(), "valid_token");
    }

    #[test]
    fn test_new_empty_token() {
        assert!(matches!(
            OAuthAccessToken::new(""),
            Err(AccessTokenError::Empty)
        ));
    }

    #[test]
    fn test_display() {
        let token = OAuthAccessToken::new("test_token").unwrap();
        assert_eq!(token.to_string(), "test_token");
    }

    #[test]
    fn test_into_inner() {
        let token = OAuthAccessToken::new("test_token").unwrap();
        assert_eq!(token.into_inner(), "test_token");
    }

    #[test]
    fn test_as_ref() {
        let token = OAuthAccessToken::new("test_token").unwrap();
        assert_eq!(token.as_ref(), "test_token");
    }
}