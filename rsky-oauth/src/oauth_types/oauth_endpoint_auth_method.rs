use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Authentication methods supported at OAuth endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OAuthEndpointAuthMethod {
    /// Client credentials in the Authorization header using HTTP Basic auth
    ClientSecretBasic,
    /// Client credentials using JWT assertion
    ClientSecretJwt,
    /// Client credentials in the POST body
    ClientSecretPost,

    /// No authentication
    #[serde(rename = "none")]
    None,
    /// Private key JWT authentication
    PrivateKeyJwt,
    /// Self-signed TLS client authentication
    SelfSignedTlsClientAuth,
    /// TLS client authentication
    TlsClientAuth,
}

impl OAuthEndpointAuthMethod {
    /// Get a slice of all possible authentication methods
    pub fn variants() -> &'static [OAuthEndpointAuthMethod] {
        &[
            OAuthEndpointAuthMethod::ClientSecretBasic,
            OAuthEndpointAuthMethod::ClientSecretJwt,
            OAuthEndpointAuthMethod::ClientSecretPost,
            OAuthEndpointAuthMethod::None,
            OAuthEndpointAuthMethod::PrivateKeyJwt,
            OAuthEndpointAuthMethod::SelfSignedTlsClientAuth,
            OAuthEndpointAuthMethod::TlsClientAuth,
        ]
    }

    /// Convert to the standard string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            OAuthEndpointAuthMethod::ClientSecretBasic => "client_secret_basic",
            OAuthEndpointAuthMethod::ClientSecretJwt => "client_secret_jwt",
            OAuthEndpointAuthMethod::ClientSecretPost => "client_secret_post",
            OAuthEndpointAuthMethod::None => "none",
            OAuthEndpointAuthMethod::PrivateKeyJwt => "private_key_jwt",
            OAuthEndpointAuthMethod::SelfSignedTlsClientAuth => "self_signed_tls_client_auth",
            OAuthEndpointAuthMethod::TlsClientAuth => "tls_client_auth",
        }
    }
}

impl Default for OAuthEndpointAuthMethod {
    fn default() -> Self {
        Self::None
    }
}

impl fmt::Display for OAuthEndpointAuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing a string into an OAuthEndpointAuthMethod fails.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Invalid endpoint authentication method: {0}")]
pub struct ParseEndpointAuthMethodError(String);

impl FromStr for OAuthEndpointAuthMethod {
    type Err = ParseEndpointAuthMethodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "client_secret_basic" => Ok(OAuthEndpointAuthMethod::ClientSecretBasic),
            "client_secret_jwt" => Ok(OAuthEndpointAuthMethod::ClientSecretJwt),
            "client_secret_post" => Ok(OAuthEndpointAuthMethod::ClientSecretPost),
            "none" => Ok(OAuthEndpointAuthMethod::None),
            "private_key_jwt" => Ok(OAuthEndpointAuthMethod::PrivateKeyJwt),
            "self_signed_tls_client_auth" => Ok(OAuthEndpointAuthMethod::SelfSignedTlsClientAuth),
            "tls_client_auth" => Ok(OAuthEndpointAuthMethod::TlsClientAuth),
            _ => Err(ParseEndpointAuthMethodError(s.to_string())),
        }
    }
}

impl AsRef<str> for OAuthEndpointAuthMethod {
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
        let variants = OAuthEndpointAuthMethod::variants();
        assert_eq!(variants.len(), 7);
    }

    #[test]
    fn test_display() {
        assert_eq!(
            OAuthEndpointAuthMethod::ClientSecretBasic.to_string(),
            "client_secret_basic"
        );
        assert_eq!(OAuthEndpointAuthMethod::None.to_string(), "none");
        assert_eq!(
            OAuthEndpointAuthMethod::TlsClientAuth.to_string(),
            "tls_client_auth"
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "client_secret_basic"
                .parse::<OAuthEndpointAuthMethod>()
                .unwrap(),
            OAuthEndpointAuthMethod::ClientSecretBasic
        );
        assert_eq!(
            "none".parse::<OAuthEndpointAuthMethod>().unwrap(),
            OAuthEndpointAuthMethod::None
        );

        assert!("invalid".parse::<OAuthEndpointAuthMethod>().is_err());
    }

    #[test]
    fn test_as_ref() {
        assert_eq!(
            OAuthEndpointAuthMethod::ClientSecretBasic.as_ref(),
            "client_secret_basic"
        );
        assert_eq!(OAuthEndpointAuthMethod::None.as_ref(), "none");
    }

    #[test]
    fn test_default() {
        assert_eq!(
            OAuthEndpointAuthMethod::default(),
            OAuthEndpointAuthMethod::None
        );
    }

    #[test]
    fn test_hash() {
        let mut set = HashSet::new();
        set.insert(OAuthEndpointAuthMethod::ClientSecretBasic);
        set.insert(OAuthEndpointAuthMethod::None);

        assert!(set.contains(&OAuthEndpointAuthMethod::ClientSecretBasic));
        assert!(!set.contains(&OAuthEndpointAuthMethod::TlsClientAuth));
    }

    #[test]
    fn test_clone_and_copy() {
        let method = OAuthEndpointAuthMethod::None;
        let cloned = method.clone();
        assert_eq!(method, cloned);

        let copied = method;
        assert_eq!(method, copied);
    }
}
