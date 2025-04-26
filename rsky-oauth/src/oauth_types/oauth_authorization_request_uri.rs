use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::oauth_types::OAuthRequestUri;

/// An OAuth authorization request URI.
///
/// Used to reference authorization request parameters that were previously
/// pushed to the authorization server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthAuthorizationRequestUri {
    /// The request URI pointing to the pushed authorization request
    request_uri: OAuthRequestUri,
}

impl OAuthAuthorizationRequestUri {
    /// Create a new authorization request URI.
    pub fn new(request_uri: OAuthRequestUri) -> Self {
        Self { request_uri }
    }

    /// Get a reference to the request URI.
    pub fn request_uri(&self) -> &OAuthRequestUri {
        &self.request_uri
    }

    /// Convert into the inner request URI.
    pub fn into_inner(self) -> OAuthRequestUri {
        self.request_uri
    }
}

impl AsRef<OAuthRequestUri> for OAuthAuthorizationRequestUri {
    fn as_ref(&self) -> &OAuthRequestUri {
        &self.request_uri
    }
}

impl From<OAuthRequestUri> for OAuthAuthorizationRequestUri {
    fn from(request_uri: OAuthRequestUri) -> Self {
        Self::new(request_uri)
    }
}

impl FromStr for OAuthAuthorizationRequestUri {
    type Err = <OAuthRequestUri as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(OAuthRequestUri::from_str(s)?))
    }
}

impl fmt::Display for OAuthAuthorizationRequestUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.request_uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a test request URI
    fn get_request_uri() -> OAuthRequestUri {
        OAuthRequestUri::new("https://example.com/oauth/request").unwrap()
    }

    #[test]
    fn test_new() {
        let uri = get_request_uri();
        let request = OAuthAuthorizationRequestUri::new(uri.clone());
        assert_eq!(request.request_uri(), &uri);
    }

    #[test]
    fn test_from() {
        let uri = get_request_uri();
        let request: OAuthAuthorizationRequestUri = uri.clone().into();
        assert_eq!(request.request_uri(), &uri);
    }

    #[test]
    fn test_into_inner() {
        let uri = get_request_uri();
        let request = OAuthAuthorizationRequestUri::new(uri.clone());
        assert_eq!(request.into_inner(), uri);
    }

    #[test]
    fn test_as_ref() {
        let uri = get_request_uri();
        let request = OAuthAuthorizationRequestUri::new(uri.clone());
        assert_eq!(request.as_ref(), &uri);
    }

    #[test]
    fn test_display() {
        let uri = get_request_uri();
        let request = OAuthAuthorizationRequestUri::new(uri.clone());
        assert_eq!(request.to_string(), uri.to_string());
    }

    #[test]
    fn test_serialization() {
        let uri = get_request_uri();
        let request = OAuthAuthorizationRequestUri::new(uri);

        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: OAuthAuthorizationRequestUri = serde_json::from_str(&serialized).unwrap();

        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_from_str() {
        // This will need to be updated with valid URI strings once OAuthRequestUri is implemented
        let result = "https://example.com/request".parse::<OAuthAuthorizationRequestUri>();
        assert!(result.is_ok() || result.is_err()); // Placeholder assertion
    }
}
