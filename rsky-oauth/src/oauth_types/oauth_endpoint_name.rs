use std::fmt;
use std::str::FromStr;

/// Names of standard OAuth endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OAuthEndpointName {
    /// Token endpoint for obtaining access tokens
    Token,
    /// Revocation endpoint for revoking tokens
    Revocation,
    /// Introspection endpoint for validating tokens
    Introspection,
    /// Pushed authorization request endpoint
    PushedAuthorizationRequest,
}

impl OAuthEndpointName {
    /// Get a slice of all possible endpoint names
    pub fn variants() -> &'static [OAuthEndpointName] {
        &[
            OAuthEndpointName::Token,
            OAuthEndpointName::Revocation,
            OAuthEndpointName::Introspection,
            OAuthEndpointName::PushedAuthorizationRequest,
        ]
    }

    /// Convert to a string representation used in configuration
    pub fn as_config_str(&self) -> &'static str {
        match self {
            OAuthEndpointName::Token => "token",
            OAuthEndpointName::Revocation => "revocation",
            OAuthEndpointName::Introspection => "introspection",
            OAuthEndpointName::PushedAuthorizationRequest => "pushed_authorization_request",
        }
    }
}

impl fmt::Display for OAuthEndpointName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_config_str())
    }
}

/// Error returned when parsing a string into an OAuthEndpointName fails.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Invalid endpoint name: {0}")]
pub struct ParseEndpointNameError(String);

impl FromStr for OAuthEndpointName {
    type Err = ParseEndpointNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "token" => Ok(OAuthEndpointName::Token),
            "revocation" => Ok(OAuthEndpointName::Revocation),
            "introspection" => Ok(OAuthEndpointName::Introspection),
            "pushed_authorization_request" => Ok(OAuthEndpointName::PushedAuthorizationRequest),
            _ => Err(ParseEndpointNameError(s.to_string())),
        }
    }
}

impl AsRef<str> for OAuthEndpointName {
    fn as_ref(&self) -> &str {
        self.as_config_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_variants() {
        let variants = OAuthEndpointName::variants();
        assert_eq!(variants.len(), 4);
        assert!(variants.contains(&OAuthEndpointName::Token));
        assert!(variants.contains(&OAuthEndpointName::Revocation));
        assert!(variants.contains(&OAuthEndpointName::Introspection));
        assert!(variants.contains(&OAuthEndpointName::PushedAuthorizationRequest));
    }

    #[test]
    fn test_display() {
        assert_eq!(OAuthEndpointName::Token.to_string(), "token");
        assert_eq!(OAuthEndpointName::Revocation.to_string(), "revocation");
        assert_eq!(OAuthEndpointName::Introspection.to_string(), "introspection");
        assert_eq!(
            OAuthEndpointName::PushedAuthorizationRequest.to_string(),
            "pushed_authorization_request"
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!("token".parse::<OAuthEndpointName>().unwrap(), OAuthEndpointName::Token);
        assert_eq!("TOKEN".parse::<OAuthEndpointName>().unwrap(), OAuthEndpointName::Token);
        assert_eq!("revocation".parse::<OAuthEndpointName>().unwrap(), OAuthEndpointName::Revocation);
        assert_eq!("introspection".parse::<OAuthEndpointName>().unwrap(), OAuthEndpointName::Introspection);
        assert_eq!(
            "pushed_authorization_request".parse::<OAuthEndpointName>().unwrap(),
            OAuthEndpointName::PushedAuthorizationRequest
        );
        
        assert!("invalid".parse::<OAuthEndpointName>().is_err());
    }

    #[test]
    fn test_as_ref() {
        assert_eq!(OAuthEndpointName::Token.as_ref(), "token");
        assert_eq!(OAuthEndpointName::Revocation.as_ref(), "revocation");
        assert_eq!(OAuthEndpointName::Introspection.as_ref(), "introspection");
        assert_eq!(
            OAuthEndpointName::PushedAuthorizationRequest.as_ref(),
            "pushed_authorization_request"
        );
    }

    #[test]
    fn test_hash() {
        let mut set = HashSet::new();
        set.insert(OAuthEndpointName::Token);
        set.insert(OAuthEndpointName::Revocation);
        
        assert!(set.contains(&OAuthEndpointName::Token));
        assert!(!set.contains(&OAuthEndpointName::Introspection));
    }
}