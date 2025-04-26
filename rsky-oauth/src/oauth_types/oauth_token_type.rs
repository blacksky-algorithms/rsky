use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Represents the type of OAuth token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OAuthTokenType {
    /// DPoP token type
    DPoP,
    /// Bearer token type
    Bearer,
}

impl fmt::Display for OAuthTokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OAuthTokenType::DPoP => write!(f, "DPoP"),
            OAuthTokenType::Bearer => write!(f, "Bearer"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid token type: {0}")]
pub struct ParseTokenTypeError(String);

impl FromStr for OAuthTokenType {
    type Err = ParseTokenTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dpop" => Ok(OAuthTokenType::DPoP),
            "bearer" => Ok(OAuthTokenType::Bearer),
            _ => Err(ParseTokenTypeError(s.to_string())),
        }
    }
}

impl AsRef<str> for OAuthTokenType {
    fn as_ref(&self) -> &str {
        match self {
            OAuthTokenType::DPoP => "DPoP",
            OAuthTokenType::Bearer => "Bearer",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::AsRef;

    #[test]
    fn test_display() {
        assert_eq!(OAuthTokenType::DPoP.to_string(), "DPoP");
        assert_eq!(OAuthTokenType::Bearer.to_string(), "Bearer");
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "dpop".parse::<OAuthTokenType>().unwrap(),
            OAuthTokenType::DPoP
        );
        assert_eq!(
            "DPoP".parse::<OAuthTokenType>().unwrap(),
            OAuthTokenType::DPoP
        );
        assert_eq!(
            "bearer".parse::<OAuthTokenType>().unwrap(),
            OAuthTokenType::Bearer
        );
        assert_eq!(
            "Bearer".parse::<OAuthTokenType>().unwrap(),
            OAuthTokenType::Bearer
        );

        assert!("invalid".parse::<OAuthTokenType>().is_err());
    }

    #[test]
    fn test_as_ref() {
        assert_eq!(OAuthTokenType::DPoP.as_ref(), "DPoP");
        assert_eq!(OAuthTokenType::Bearer.as_ref(), "Bearer");
    }
}
