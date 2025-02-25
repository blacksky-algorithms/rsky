use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// OAuth grant types, including standard grants and extension grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OAuthGrantType {
    /// Authorization code grant
    AuthorizationCode,
    /// Implicit grant (deprecated in OAuth 2.1)
    Implicit,
    /// Refresh token grant
    RefreshToken,
    /// Resource owner password credentials grant (not in OAuth 2.1)
    Password,
    /// Client credentials grant
    ClientCredentials,
    /// JWT bearer token grant
    JwtBearer,
    /// SAML 2.0 bearer token grant
    Saml2Bearer,
}

impl OAuthGrantType {
    /// Get a slice of all possible grant types
    pub fn variants() -> &'static [OAuthGrantType] {
        &[
            OAuthGrantType::AuthorizationCode,
            OAuthGrantType::Implicit,
            OAuthGrantType::RefreshToken,
            OAuthGrantType::Password,
            OAuthGrantType::ClientCredentials,
            OAuthGrantType::JwtBearer,
            OAuthGrantType::Saml2Bearer,
        ]
    }

    /// Get the URI identifier for this grant type
    pub fn uri(&self) -> &'static str {
        match self {
            OAuthGrantType::AuthorizationCode => "authorization_code",
            OAuthGrantType::Implicit => "implicit",
            OAuthGrantType::RefreshToken => "refresh_token",
            OAuthGrantType::Password => "password",
            OAuthGrantType::ClientCredentials => "client_credentials",
            OAuthGrantType::JwtBearer => "urn:ietf:params:oauth:grant-type:jwt-bearer",
            OAuthGrantType::Saml2Bearer => "urn:ietf:params:oauth:grant-type:saml2-bearer",
        }
    }

    /// Returns true if this grant type is supported in OAuth 2.1
    pub fn is_oauth2_1_compliant(&self) -> bool {
        !matches!(self, OAuthGrantType::Implicit | OAuthGrantType::Password)
    }
}

impl fmt::Display for OAuthGrantType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.uri())
    }
}

/// Error returned when parsing a string into an OAuthGrantType fails.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Invalid grant type: {0}")]
pub struct ParseGrantTypeError(String);

impl FromStr for OAuthGrantType {
    type Err = ParseGrantTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "authorization_code" => Ok(OAuthGrantType::AuthorizationCode),
            "implicit" => Ok(OAuthGrantType::Implicit),
            "refresh_token" => Ok(OAuthGrantType::RefreshToken),
            "password" => Ok(OAuthGrantType::Password),
            "client_credentials" => Ok(OAuthGrantType::ClientCredentials),
            "urn:ietf:params:oauth:grant-type:jwt-bearer" => Ok(OAuthGrantType::JwtBearer),
            "urn:ietf:params:oauth:grant-type:saml2-bearer" => Ok(OAuthGrantType::Saml2Bearer),
            _ => Err(ParseGrantTypeError(s.to_string())),
        }
    }
}

impl AsRef<str> for OAuthGrantType {
    fn as_ref(&self) -> &str {
        self.uri()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_variants() {
        let variants = OAuthGrantType::variants();
        assert_eq!(variants.len(), 7);
        assert!(variants.contains(&OAuthGrantType::AuthorizationCode));
        assert!(variants.contains(&OAuthGrantType::JwtBearer));
    }

    #[test]
    fn test_display() {
        assert_eq!(
            OAuthGrantType::AuthorizationCode.to_string(),
            "authorization_code"
        );
        assert_eq!(
            OAuthGrantType::JwtBearer.to_string(),
            "urn:ietf:params:oauth:grant-type:jwt-bearer"
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "authorization_code".parse::<OAuthGrantType>().unwrap(),
            OAuthGrantType::AuthorizationCode
        );
        assert_eq!(
            "urn:ietf:params:oauth:grant-type:jwt-bearer"
                .parse::<OAuthGrantType>()
                .unwrap(),
            OAuthGrantType::JwtBearer
        );

        assert!("invalid".parse::<OAuthGrantType>().is_err());
    }

    #[test]
    fn test_as_ref() {
        assert_eq!(
            OAuthGrantType::AuthorizationCode.as_ref(),
            "authorization_code"
        );
        assert_eq!(
            OAuthGrantType::JwtBearer.as_ref(),
            "urn:ietf:params:oauth:grant-type:jwt-bearer"
        );
    }

    #[test]
    fn test_is_oauth2_1_compliant() {
        assert!(OAuthGrantType::AuthorizationCode.is_oauth2_1_compliant());
        assert!(OAuthGrantType::ClientCredentials.is_oauth2_1_compliant());
        assert!(!OAuthGrantType::Implicit.is_oauth2_1_compliant());
        assert!(!OAuthGrantType::Password.is_oauth2_1_compliant());
    }

    #[test]
    fn test_hash() {
        let mut set = HashSet::new();
        set.insert(OAuthGrantType::AuthorizationCode);
        set.insert(OAuthGrantType::ClientCredentials);

        assert!(set.contains(&OAuthGrantType::AuthorizationCode));
        assert!(!set.contains(&OAuthGrantType::Password));
    }

    #[test]
    fn test_clone_and_copy() {
        let grant_type = OAuthGrantType::AuthorizationCode;
        let cloned = grant_type.clone();
        assert_eq!(grant_type, cloned);

        let copied = grant_type;
        assert_eq!(grant_type, copied);
    }
}
