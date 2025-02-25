use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth_types::{
    OAuthAuthorizationRequestJar, OAuthAuthorizationRequestParameters, OAuthClientId,
    OAuthRedirectUri, OAuthResponseType, OAuthScope,
};

/// A Pushed Authorization Request (PAR).
///
/// PAR allows clients to push authorization request parameters directly to the
/// authorization server rather than passing them through the browser. See RFC 9126.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OAuthAuthorizationRequestPar {
    /// Standard authorization request parameters
    Parameters(OAuthAuthorizationRequestParameters),
    /// JWT-based request parameters (JAR)
    Jar(OAuthAuthorizationRequestJar),
}

impl From<OAuthAuthorizationRequestParameters> for OAuthAuthorizationRequestPar {
    fn from(params: OAuthAuthorizationRequestParameters) -> Self {
        Self::Parameters(params)
    }
}

impl From<OAuthAuthorizationRequestJar> for OAuthAuthorizationRequestPar {
    fn from(jar: OAuthAuthorizationRequestJar) -> Self {
        Self::Jar(jar)
    }
}

impl OAuthAuthorizationRequestPar {
    /// Returns true if this is a parameters-based request.
    pub fn is_parameters(&self) -> bool {
        matches!(self, Self::Parameters(_))
    }

    /// Returns true if this is a JAR-based request.
    pub fn is_jar(&self) -> bool {
        matches!(self, Self::Jar(_))
    }

    /// Get the inner parameters if this is a parameters-based request.
    pub fn as_parameters(&self) -> Option<&OAuthAuthorizationRequestParameters> {
        match self {
            Self::Parameters(params) => Some(params),
            _ => None,
        }
    }

    /// Get the inner JAR if this is a JAR-based request.
    pub fn as_jar(&self) -> Option<&OAuthAuthorizationRequestJar> {
        match self {
            Self::Jar(jar) => Some(jar),
            _ => None,
        }
    }
}

impl fmt::Display for OAuthAuthorizationRequestPar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parameters(_) => write!(f, "PAR(parameters)"),
            Self::Jar(_) => write!(f, "PAR(jar)"),
        }
    }
}

/// Errors that can occur with PAR requests.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParRequestError {
    #[error("Invalid form encoding")]
    InvalidFormEncoding,

    #[error("Invalid JAR: {0}")]
    InvalidJar(#[source] crate::oauth_types::JarError),
    // Feel free to add more error types if need
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_types::{OAuthClientId, RequestClaims};
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Helper function to create test parameters
    fn test_parameters() -> OAuthAuthorizationRequestParameters {
        let client_id = OAuthClientId::new("test_client").unwrap();
        let response_type = OAuthResponseType::Code;
        let redirect_uri = OAuthRedirectUri::new("https://example.com/callback").unwrap();
        let scope = OAuthScope::new("read write").unwrap();
        let state = None;
        OAuthAuthorizationRequestParameters::new(
            client_id,
            response_type,
            Some(redirect_uri),
            Some(scope),
            state,
        )
        .unwrap()
    }

    // Helper function to create test JAR
    fn test_jar() -> OAuthAuthorizationRequestJar {
        let test_secret = b"test_secret".as_ref();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let json_claims = serde_json::json!({
            "iat": now,
            "exp": now + 300,
            "client_id": "test_client",
            "response_type": "code",
            "redirect_uri": "https://example.com/callback",
            "scope": "read write"
        });

        // Then convert it to RequestClaims
        let claims: RequestClaims =
            serde_json::from_value(json_claims).expect("Failed to convert JSON to RequestClaims");

        OAuthAuthorizationRequestJar::new(claims, Some(Algorithm::HS256), Some(test_secret))
            .unwrap()
    }

    #[test]
    fn test_from_implementations() {
        let params = test_parameters();
        let par: OAuthAuthorizationRequestPar = params.clone().into();
        assert!(par.is_parameters());
        assert_eq!(par.as_parameters().unwrap(), &params);

        let jar = test_jar();
        let par: OAuthAuthorizationRequestPar = jar.clone().into();
        assert!(par.is_jar());
        assert_eq!(par.as_jar().unwrap(), &jar);
    }

    #[test]
    fn test_type_checks() {
        let par: OAuthAuthorizationRequestPar = test_parameters().into();
        assert!(par.is_parameters());
        assert!(!par.is_jar());
        assert!(par.as_parameters().is_some());
        assert!(par.as_jar().is_none());

        let par: OAuthAuthorizationRequestPar = test_jar().into();
        assert!(!par.is_parameters());
        assert!(par.is_jar());
        assert!(par.as_parameters().is_none());
        assert!(par.as_jar().is_some());
    }

    #[test]
    fn test_display() {
        let par: OAuthAuthorizationRequestPar = test_parameters().into();
        assert_eq!(par.to_string(), "PAR(parameters)");

        let par: OAuthAuthorizationRequestPar = test_jar().into();
        assert_eq!(par.to_string(), "PAR(jar)");
    }

    #[test]
    fn test_serialization() {
        let par: OAuthAuthorizationRequestPar = test_parameters().into();
        let serialized = serde_json::to_string(&par).unwrap();
        let deserialized: OAuthAuthorizationRequestPar = serde_json::from_str(&serialized).unwrap();
        assert_eq!(par, deserialized);

        let par: OAuthAuthorizationRequestPar = test_jar().into();
        let serialized = serde_json::to_string(&par).unwrap();
        let deserialized: OAuthAuthorizationRequestPar = serde_json::from_str(&serialized).unwrap();
        assert_eq!(par, deserialized);
    }
}
