use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The response type for an OAuth authorization request.
///
/// This includes OAuth2 standard response types and OpenID Connect composite response types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OAuthResponseType {
    /// Authorization Code Grant type (OAuth2)
    Code,
    /// Implicit Grant type (OAuth2)
    Token,
    /// No response type (OpenID Connect)
    None,
    /// Code, ID token, and access token (OpenID Connect)
    CodeIdTokenToken,
    /// Code and ID token (OpenID Connect)
    CodeIdToken,
    /// Code and access token (OpenID Connect)
    CodeToken,
    /// ID token and access token (OpenID Connect)
    IdTokenToken,
    /// ID token only (OpenID Connect)
    IdToken,
}

impl OAuthResponseType {
    /// Get a slice of all possible response types
    pub fn variants() -> &'static [OAuthResponseType] {
        &[
            OAuthResponseType::Code,
            OAuthResponseType::Token,
            OAuthResponseType::None,
            OAuthResponseType::CodeIdTokenToken,
            OAuthResponseType::CodeIdToken,
            OAuthResponseType::CodeToken,
            OAuthResponseType::IdTokenToken,
            OAuthResponseType::IdToken,
        ]
    }

    /// Returns true if this response type includes a code
    pub fn includes_code(&self) -> bool {
        matches!(
            self,
            OAuthResponseType::Code
                | OAuthResponseType::CodeIdTokenToken
                | OAuthResponseType::CodeIdToken
                | OAuthResponseType::CodeToken
        )
    }

    /// Returns true if this response type includes an id_token
    pub fn includes_id_token(&self) -> bool {
        matches!(
            self,
            OAuthResponseType::CodeIdTokenToken
                | OAuthResponseType::CodeIdToken
                | OAuthResponseType::IdTokenToken
                | OAuthResponseType::IdToken
        )
    }

    /// Returns true if this response type includes an access token
    pub fn includes_token(&self) -> bool {
        matches!(
            self,
            OAuthResponseType::Token
                | OAuthResponseType::CodeIdTokenToken
                | OAuthResponseType::CodeToken
                | OAuthResponseType::IdTokenToken
        )
    }

    /// Convert to the standard string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            OAuthResponseType::Code => "code",
            OAuthResponseType::Token => "token",
            OAuthResponseType::None => "none",
            OAuthResponseType::CodeIdTokenToken => "code id_token token",
            OAuthResponseType::CodeIdToken => "code id_token",
            OAuthResponseType::CodeToken => "code token",
            OAuthResponseType::IdTokenToken => "id_token token",
            OAuthResponseType::IdToken => "id_token",
        }
    }
}

impl fmt::Display for OAuthResponseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing a string into an OAuthResponseType fails.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Invalid response type: {0}")]
pub struct ParseResponseTypeError(String);

impl FromStr for OAuthResponseType {
    type Err = ParseResponseTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Normalize input by sorting space-separated parts
        let mut parts: Vec<&str> = s.split_whitespace().collect();
        parts.sort_unstable();
        let normalized = parts.join(" ");

        match normalized.as_str() {
            "code" => Ok(OAuthResponseType::Code),
            "token" => Ok(OAuthResponseType::Token),
            "none" => Ok(OAuthResponseType::None),
            "code id_token token" => Ok(OAuthResponseType::CodeIdTokenToken),
            "code id_token" => Ok(OAuthResponseType::CodeIdToken),
            "code token" => Ok(OAuthResponseType::CodeToken),
            "id_token token" => Ok(OAuthResponseType::IdTokenToken),
            "id_token" => Ok(OAuthResponseType::IdToken),
            _ => Err(ParseResponseTypeError(s.to_string())),
        }
    }
}

impl AsRef<str> for OAuthResponseType {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_variants() {
        let variants = OAuthResponseType::variants();
        assert_eq!(variants.len(), 8);
    }

    #[test]
    fn test_display() {
        assert_eq!(OAuthResponseType::Code.to_string(), "code");
        assert_eq!(
            OAuthResponseType::CodeIdTokenToken.to_string(),
            "code id_token token"
        );
    }

    #[test]
    fn test_from_str() {
        // Test standard formats
        assert_eq!(
            "code".parse::<OAuthResponseType>().unwrap(),
            OAuthResponseType::Code
        );
        assert_eq!(
            "token".parse::<OAuthResponseType>().unwrap(),
            OAuthResponseType::Token
        );

        // Test order-independent parsing
        assert_eq!(
            "token id_token code".parse::<OAuthResponseType>().unwrap(),
            OAuthResponseType::CodeIdTokenToken
        );
        assert_eq!(
            "id_token code".parse::<OAuthResponseType>().unwrap(),
            OAuthResponseType::CodeIdToken
        );

        // Test invalid
        assert!("invalid".parse::<OAuthResponseType>().is_err());
    }

    #[test]
    fn test_includes_methods() {
        let response_type = OAuthResponseType::CodeIdTokenToken;
        assert!(response_type.includes_code());
        assert!(response_type.includes_id_token());
        assert!(response_type.includes_token());

        let response_type = OAuthResponseType::Code;
        assert!(response_type.includes_code());
        assert!(!response_type.includes_id_token());
        assert!(!response_type.includes_token());
    }

    #[test]
    fn test_as_ref() {
        assert_eq!(OAuthResponseType::Code.as_ref(), "code");
        assert_eq!(
            OAuthResponseType::CodeIdTokenToken.as_ref(),
            "code id_token token"
        );
    }

    #[test]
    fn test_hash() {
        let mut set = HashSet::new();
        set.insert(OAuthResponseType::Code);
        set.insert(OAuthResponseType::Token);

        assert!(set.contains(&OAuthResponseType::Code));
        assert!(!set.contains(&OAuthResponseType::IdToken));
    }

    #[test]
    fn test_clone_and_copy() {
        let response_type = OAuthResponseType::Code;
        let cloned = response_type.clone();
        assert_eq!(response_type, cloned);

        let copied = response_type;
        assert_eq!(response_type, copied);
    }
}
