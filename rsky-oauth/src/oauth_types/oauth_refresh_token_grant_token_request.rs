use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use url::form_urlencoded;

use crate::oauth_types::{GrantType, OAuthRefreshToken};

/// A refresh token grant token request.
///
/// Used to obtain a new access token (and optionally a new refresh token)
/// using a refresh token from a previous authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthRefreshTokenGrantTokenRequest {
    /// Must be "refresh_token"
    grant_type: GrantType,

    /// The refresh token from a previous authorization
    refresh_token: OAuthRefreshToken,
}

impl OAuthRefreshTokenGrantTokenRequest {
    /// Create a new refresh token grant request.
    pub fn new(refresh_token: OAuthRefreshToken) -> Self {
        Self {
            grant_type: GrantType::RefreshToken,
            refresh_token,
        }
    }

    /// Get the refresh token.
    pub fn refresh_token(&self) -> &OAuthRefreshToken {
        &self.refresh_token
    }

    /// Convert the request to form-urlencoded parameters.
    pub fn to_form_urlencoded(&self) -> String {
        let params = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", self.refresh_token.as_ref()),
        ];

        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish()
    }

    /// Parse from form-urlencoded parameters.
    pub fn from_form_urlencoded(form: &str) -> Result<Self, RefreshTokenGrantError> {
        let mut refresh_token = None;
        let mut found_grant_type = false;

        for (key, value) in form_urlencoded::parse(form.as_bytes()) {
            match key.as_ref() {
                "grant_type" => {
                    if value != "refresh_token" {
                        return Err(RefreshTokenGrantError::InvalidGrantType);
                    }
                    found_grant_type = true;
                }
                "refresh_token" => {
                    refresh_token = Some(OAuthRefreshToken::new(value.into_owned())?);
                }
                _ => {} // Ignore unknown parameters
            }
        }

        if !found_grant_type {
            return Err(RefreshTokenGrantError::MissingGrantType);
        }

        let refresh_token = refresh_token.ok_or(RefreshTokenGrantError::MissingRefreshToken)?;

        Ok(Self::new(refresh_token))
    }
}

// Custom serialization to output the correct JSON structure
impl Serialize for OAuthRefreshTokenGrantTokenRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("grant_type", "refresh_token")?;
        map.serialize_entry("refresh_token", self.refresh_token.as_ref())?;
        map.end()
    }
}

// Custom deserialization to verify the grant_type value
impl<'de> Deserialize<'de> for OAuthRefreshTokenGrantTokenRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            grant_type: String,
            refresh_token: String,
        }

        let helper = Helper::deserialize(deserializer)?;

        if helper.grant_type != "refresh_token" {
            return Err(serde::de::Error::custom(format!(
                "Invalid grant_type: expected 'refresh_token', got '{}'",
                helper.grant_type
            )));
        }

        let refresh_token = OAuthRefreshToken::new(helper.refresh_token)
            .map_err(|e| serde::de::Error::custom(format!("Invalid refresh token: {}", e)))?;

        Ok(OAuthRefreshTokenGrantTokenRequest {
            grant_type: GrantType::RefreshToken,
            refresh_token,
        })
    }
}

impl fmt::Display for OAuthRefreshTokenGrantTokenRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RefreshTokenGrant(token={})", self.refresh_token)
    }
}

/// Errors that can occur with refresh token grant requests.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RefreshTokenGrantError {
    #[error("Missing grant_type parameter")]
    MissingGrantType,

    #[error("Invalid grant_type value")]
    InvalidGrantType,

    #[error("Missing refresh_token parameter")]
    MissingRefreshToken,

    #[error("Refresh token error: {0}")]
    RefreshToken(#[from] crate::oauth_types::RefreshTokenError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_refresh_token() -> OAuthRefreshToken {
        OAuthRefreshToken::new("test_refresh_token").unwrap()
    }

    #[test]
    fn test_new_request() {
        let request = OAuthRefreshTokenGrantTokenRequest::new(test_refresh_token());
        assert_eq!(request.refresh_token().as_ref(), "test_refresh_token");
    }

    #[test]
    fn test_form_urlencoded() {
        let request = OAuthRefreshTokenGrantTokenRequest::new(test_refresh_token());
        let form = request.to_form_urlencoded();

        assert!(form.contains("grant_type=refresh_token"));
        assert!(form.contains("refresh_token=test_refresh_token"));

        let parsed = OAuthRefreshTokenGrantTokenRequest::from_form_urlencoded(&form).unwrap();
        assert_eq!(parsed, request);
    }

    #[test]
    fn test_invalid_form() {
        // Missing grant_type
        let form = "refresh_token=test";
        assert!(matches!(
            OAuthRefreshTokenGrantTokenRequest::from_form_urlencoded(form),
            Err(RefreshTokenGrantError::MissingGrantType)
        ));

        // Invalid grant_type
        let form = "grant_type=invalid&refresh_token=test";
        assert!(matches!(
            OAuthRefreshTokenGrantTokenRequest::from_form_urlencoded(form),
            Err(RefreshTokenGrantError::InvalidGrantType)
        ));

        // Missing refresh_token
        let form = "grant_type=refresh_token";
        assert!(matches!(
            OAuthRefreshTokenGrantTokenRequest::from_form_urlencoded(form),
            Err(RefreshTokenGrantError::MissingRefreshToken)
        ));
    }

    #[test]
    fn test_display() {
        let request = OAuthRefreshTokenGrantTokenRequest::new(test_refresh_token());
        assert_eq!(
            request.to_string(),
            "RefreshTokenGrant(token=test_refresh_token)"
        );
    }

    #[test]
    fn test_serialization() {
        let request = OAuthRefreshTokenGrantTokenRequest::new(test_refresh_token());

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: OAuthRefreshTokenGrantTokenRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request, deserialized);
    }
}
