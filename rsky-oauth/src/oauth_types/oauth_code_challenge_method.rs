use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Code challenge methods for PKCE (Proof Key for Code Exchange).
///
/// RFC 7636 defines two methods:
/// - S256: SHA256 hash of the code verifier
/// - plain: The code verifier itself (less secure, not recommended)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OAuthCodeChallengeMethod {
    /// SHA256 hash of the code verifier (recommended)
    S256,
    /// The code verifier itself (not recommended)
    Plain,
}

impl OAuthCodeChallengeMethod {
    /// Get a slice of all possible methods
    pub fn variants() -> &'static [OAuthCodeChallengeMethod] {
        &[
            OAuthCodeChallengeMethod::S256,
            OAuthCodeChallengeMethod::Plain,
        ]
    }

    /// Returns true if this is the recommended S256 method
    pub fn is_recommended(&self) -> bool {
        matches!(self, OAuthCodeChallengeMethod::S256)
    }
}

impl Default for OAuthCodeChallengeMethod {
    fn default() -> Self {
        Self::S256
    }
}

impl fmt::Display for OAuthCodeChallengeMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OAuthCodeChallengeMethod::S256 => write!(f, "S256"),
            OAuthCodeChallengeMethod::Plain => write!(f, "plain"),
        }
    }
}

/// Error returned when parsing a string into an OAuthCodeChallengeMethod fails.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Invalid code challenge method: {0}")]
pub struct ParseCodeChallengeMethodError(String);

impl FromStr for OAuthCodeChallengeMethod {
    type Err = ParseCodeChallengeMethodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "S256" => Ok(OAuthCodeChallengeMethod::S256),
            "plain" => Ok(OAuthCodeChallengeMethod::Plain),
            _ => Err(ParseCodeChallengeMethodError(s.to_string())),
        }
    }
}

impl AsRef<str> for OAuthCodeChallengeMethod {
    fn as_ref(&self) -> &str {
        match self {
            OAuthCodeChallengeMethod::S256 => "S256",
            OAuthCodeChallengeMethod::Plain => "plain",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_variants() {
        let variants = OAuthCodeChallengeMethod::variants();
        assert_eq!(variants.len(), 2);
        assert!(variants.contains(&OAuthCodeChallengeMethod::S256));
        assert!(variants.contains(&OAuthCodeChallengeMethod::Plain));
    }

    #[test]
    fn test_default() {
        assert_eq!(
            OAuthCodeChallengeMethod::default(),
            OAuthCodeChallengeMethod::S256
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(OAuthCodeChallengeMethod::S256.to_string(), "S256");
        assert_eq!(OAuthCodeChallengeMethod::Plain.to_string(), "plain");
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "S256".parse::<OAuthCodeChallengeMethod>().unwrap(),
            OAuthCodeChallengeMethod::S256
        );
        assert_eq!(
            "plain".parse::<OAuthCodeChallengeMethod>().unwrap(),
            OAuthCodeChallengeMethod::Plain
        );

        assert!("invalid".parse::<OAuthCodeChallengeMethod>().is_err());
        assert!("s256".parse::<OAuthCodeChallengeMethod>().is_err()); // Case sensitive
    }

    #[test]
    fn test_as_ref() {
        assert_eq!(OAuthCodeChallengeMethod::S256.as_ref(), "S256");
        assert_eq!(OAuthCodeChallengeMethod::Plain.as_ref(), "plain");
    }

    #[test]
    fn test_is_recommended() {
        assert!(OAuthCodeChallengeMethod::S256.is_recommended());
        assert!(!OAuthCodeChallengeMethod::Plain.is_recommended());
    }

    #[test]
    fn test_hash() {
        let mut set = HashSet::new();
        set.insert(OAuthCodeChallengeMethod::S256);

        assert!(set.contains(&OAuthCodeChallengeMethod::S256));
        assert!(!set.contains(&OAuthCodeChallengeMethod::Plain));
    }

    #[test]
    fn test_clone_and_copy() {
        let method = OAuthCodeChallengeMethod::S256;
        let cloned = method.clone();
        assert_eq!(method, cloned);

        let copied = method;
        assert_eq!(method, copied);
    }
}
