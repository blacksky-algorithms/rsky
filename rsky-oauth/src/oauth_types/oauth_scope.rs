use std::fmt;
use std::str::FromStr;

/// A validated OAuth scope string.
/// 
/// From OAuth 2.1 spec section 1.4.1:
/// scope = scope-token *( SP scope-token )
/// scope-token = 1*( %x21 / %x23-5B / %x5D-7E )
/// 
/// This means a space-separated list of tokens where each token can contain
/// most non-control ASCII characters except backslash and double quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthScope(String);

impl OAuthScope {
    /// Create a new OAuthScope.
    ///
    /// # Errors
    /// Returns an error if the scope string is invalid according to the OAuth 2.1 spec.
    pub fn new(scope: impl Into<String>) -> Result<Self, OAuthScopeError> {
        let scope = scope.into();
        if scope.is_empty() {
            return Err(OAuthScopeError::Empty);
        }
        
        // Validate each scope token
        for token in scope.split(' ') {
            if token.is_empty() {
                return Err(OAuthScopeError::EmptyToken);
            }
            
            if !token.chars().all(|c| {
                matches!(c as u32, 0x21 | 0x23..=0x5B | 0x5D..=0x7E)
            }) {
                return Err(OAuthScopeError::InvalidCharacters(token.to_string()));
            }
        }
        
        Ok(Self(scope))
    }

    /// Get the underlying scope string.
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Get an iterator over the individual scope tokens.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.split(' ')
    }
}

impl AsRef<str> for OAuthScope {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OAuthScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Errors that can occur when creating an OAuthScope.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OAuthScopeError {
    #[error("Scope string cannot be empty")]
    Empty,
    #[error("Scope contains an empty token")]
    EmptyToken,
    #[error("Scope token contains invalid characters: {0}")]
    InvalidCharacters(String),
}

impl FromStr for OAuthScope {
    type Err = OAuthScopeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_scopes() {
        let valid_scopes = [
            "read",
            "read write",
            "read!@#$%^&*()_+-=[]{}|;:,.<>",
            "scope~`'",
        ];

        for scope in valid_scopes {
            assert!(OAuthScope::new(scope).is_ok(), "Scope should be valid: {}", scope);
        }
    }

    #[test]
    fn test_invalid_scopes() {
        let test_cases = [
            ("", OAuthScopeError::Empty),
            ("read\\write", OAuthScopeError::InvalidCharacters("read\\write".to_string())),
            ("read\"write", OAuthScopeError::InvalidCharacters("read\"write".to_string())),
            ("read  write", OAuthScopeError::EmptyToken),
        ];

        for (scope, expected_error) in test_cases {
            let result = OAuthScope::new(scope);
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), expected_error);
        }
    }

    #[test]
    fn test_iter() {
        let scope = OAuthScope::new("read write delete").unwrap();
        let tokens: Vec<&str> = scope.iter().collect();
        assert_eq!(tokens, vec!["read", "write", "delete"]);
    }

    #[test]
    fn test_display() {
        let scope = OAuthScope::new("read write").unwrap();
        assert_eq!(scope.to_string(), "read write");
    }

    #[test]
    fn test_as_ref() {
        let scope = OAuthScope::new("read write").unwrap();
        assert_eq!(scope.as_ref(), "read write");
    }

    #[test]
    fn test_from_str() {
        let scope: OAuthScope = "read write".parse().unwrap();
        assert_eq!(scope.as_ref(), "read write");
    }
}