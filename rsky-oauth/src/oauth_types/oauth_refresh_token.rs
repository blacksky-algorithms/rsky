use std::fmt;

/// A validated OAuth refresh token.
/// 
/// This is a newtype wrapper around String that ensures the token is not empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthRefreshToken(String);

impl OAuthRefreshToken {
    /// Create a new OAuthRefreshToken.
    ///
    /// # Errors
    /// Returns an error if the token string is empty.
    pub fn new(token: impl Into<String>) -> Result<Self, RefreshTokenError> {
        let token = token.into();
        if token.is_empty() {
            return Err(RefreshTokenError::Empty);
        }
        Ok(Self(token))
    }

    /// Get the underlying token string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for OAuthRefreshToken {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OAuthRefreshToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for OAuthRefreshToken {
    type Err = RefreshTokenError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur when creating an OAuthRefreshToken.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RefreshTokenError {
    #[error("Refresh token cannot be empty")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_new_valid_token() {
        let token = OAuthRefreshToken::new("valid_token").unwrap();
        assert_eq!(token.as_ref(), "valid_token");
    }

    #[test]
    fn test_new_empty_token() {
        assert!(matches!(
            OAuthRefreshToken::new(""),
            Err(RefreshTokenError::Empty)
        ));
    }

    #[test]
    fn test_display() {
        let token = OAuthRefreshToken::new("test_token").unwrap();
        assert_eq!(token.to_string(), "test_token");
    }

    #[test]
    fn test_into_inner() {
        let token = OAuthRefreshToken::new("test_token").unwrap();
        assert_eq!(token.into_inner(), "test_token");
    }

    #[test]
    fn test_as_ref() {
        let token = OAuthRefreshToken::new("test_token").unwrap();
        assert_eq!(token.as_ref(), "test_token");
    }

    #[test]
    fn test_from_str() {
        let token = OAuthRefreshToken::from_str("test_token").unwrap();
        assert_eq!(token.as_ref(), "test_token");

        assert!(OAuthRefreshToken::from_str("").is_err());
    }
}