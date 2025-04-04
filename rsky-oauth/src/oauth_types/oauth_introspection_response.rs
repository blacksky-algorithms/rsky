use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth_types::{OAuthAuthorizationDetails, OAuthClientId, OAuthTokenType};

/// Response from a token introspection endpoint.
///
/// See RFC 7662 section 2.2 for introspection response details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "active", rename_all = "snake_case")]
pub enum OAuthIntrospectionResponse {
    /// Token is active/valid
    #[serde(rename = "true")]
    Active(ActiveTokenInfo),
    /// Token is inactive/invalid
    #[serde(rename = "false")]
    Inactive,
}

/// Information about an active token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveTokenInfo {
    /// Authorized scopes for the token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Client ID that requested the token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<OAuthClientId>,

    /// Username of the resource owner
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Type of token (e.g. Bearer, DPoP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<OAuthTokenType>,

    /// Authorization details associated with the token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_details: Option<OAuthAuthorizationDetails>,

    /// Intended audience for the token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<TokenAudience>,

    /// Expiration time (as Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,

    /// Issued at time (as Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,

    /// Issuer identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// Unique token identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    /// Not before time (as Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,

    /// Subject identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
}

/// Token audience - can be a single string or array of strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TokenAudience {
    /// Single audience string
    Single(String),
    /// Multiple audience strings
    Multiple(Vec<String>),
}

impl OAuthIntrospectionResponse {
    /// Create a new response for an inactive token.
    pub fn inactive() -> Self {
        Self::Inactive
    }

    /// Create a new response for an active token.
    pub fn active() -> ActiveTokenInfoBuilder {
        ActiveTokenInfoBuilder::new()
    }

    /// Returns true if the token is active.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active(_))
    }

    /// Get the token info if the token is active.
    pub fn token_info(&self) -> Option<&ActiveTokenInfo> {
        match self {
            Self::Active(info) => Some(info),
            Self::Inactive => None,
        }
    }

    /// Convert to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Parse from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Builder for constructing active token information.
#[derive(Debug)]
pub struct ActiveTokenInfoBuilder {
    info: ActiveTokenInfo,
}

impl ActiveTokenInfoBuilder {
    fn new() -> Self {
        Self {
            info: ActiveTokenInfo {
                scope: None,
                client_id: None,
                username: None,
                token_type: None,
                authorization_details: None,
                aud: None,
                exp: None,
                iat: None,
                iss: None,
                jti: None,
                nbf: None,
                sub: None,
            },
        }
    }

    /// Add scope information.
    pub fn scope(mut self, scope: impl Into<String>) -> Self {
        self.info.scope = Some(scope.into());
        self
    }

    /// Add client ID.
    pub fn client_id(mut self, client_id: OAuthClientId) -> Self {
        self.info.client_id = Some(client_id);
        self
    }

    /// Add username.
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.info.username = Some(username.into());
        self
    }

    /// Add token type.
    pub fn token_type(mut self, token_type: OAuthTokenType) -> Self {
        self.info.token_type = Some(token_type);
        self
    }

    /// Add authorization details.
    pub fn authorization_details(mut self, details: OAuthAuthorizationDetails) -> Self {
        self.info.authorization_details = Some(details);
        self
    }

    /// Add single audience.
    pub fn audience(mut self, aud: impl Into<String>) -> Self {
        self.info.aud = Some(TokenAudience::Single(aud.into()));
        self
    }

    /// Add multiple audiences.
    pub fn audiences(mut self, audiences: Vec<String>) -> Self {
        if audiences.len() == 1 {
            self.info.aud = Some(TokenAudience::Single(audiences.into_iter().next().unwrap()));
        } else {
            self.info.aud = Some(TokenAudience::Multiple(audiences));
        }
        self
    }

    /// Add expiration time.
    pub fn expiration(mut self, exp: i64) -> Self {
        self.info.exp = Some(exp);
        self
    }

    /// Add issued at time.
    pub fn issued_at(mut self, iat: i64) -> Self {
        self.info.iat = Some(iat);
        self
    }

    /// Add issuer.
    pub fn issuer(mut self, iss: impl Into<String>) -> Self {
        self.info.iss = Some(iss.into());
        self
    }

    /// Add token ID.
    pub fn token_id(mut self, jti: impl Into<String>) -> Self {
        self.info.jti = Some(jti.into());
        self
    }

    /// Add not before time.
    pub fn not_before(mut self, nbf: i64) -> Self {
        self.info.nbf = Some(nbf);
        self
    }

    /// Add subject.
    pub fn subject(mut self, sub: impl Into<String>) -> Self {
        self.info.sub = Some(sub.into());
        self
    }

    /// Build the final response.
    pub fn build(self) -> OAuthIntrospectionResponse {
        OAuthIntrospectionResponse::Active(self.info)
    }
}

impl fmt::Display for OAuthIntrospectionResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active(info) => {
                write!(f, "Active token")?;
                if let Some(sub) = &info.sub {
                    write!(f, " for subject {}", sub)?;
                }
                if let Some(client_id) = &info.client_id {
                    write!(f, " (client: {})", client_id)?;
                }
                Ok(())
            }
            Self::Inactive => write!(f, "Inactive token"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    #[test]
    fn test_inactive_token() {
        let response = OAuthIntrospectionResponse::inactive();
        assert!(!response.is_active());
        assert!(response.token_info().is_none());
    }

    #[test]
    fn test_active_token_builder() {
        let now = Utc::now().timestamp();
        let response = OAuthIntrospectionResponse::active()
            .scope("read write")
            .client_id("test_client")
            .username("test_user")
            .token_type(OAuthTokenType::Bearer)
            .audience("https://api.example.com")
            .expiration(now + 3600)
            .issued_at(now)
            .subject("user123")
            .build();

        assert!(response.is_active());
        let info = response.token_info().unwrap();
        assert_eq!(info.scope, Some("read write".to_string()));
        assert_eq!(info.client_id, Some("test_client".to_string()));
        assert_eq!(info.username, Some("test_user".to_string()));
        assert_eq!(info.token_type, Some(OAuthTokenType::Bearer));
        assert!(matches!(info.aud, Some(TokenAudience::Single(_))));
        assert_eq!(info.exp, Some(now + 3600));
        assert_eq!(info.iat, Some(now));
        assert_eq!(info.sub, Some("user123".to_string()));
    }

    #[test]
    fn test_multiple_audiences() {
        let response = OAuthIntrospectionResponse::active()
            .audiences(vec!["aud1".to_string(), "aud2".to_string()])
            .build();

        if let OAuthIntrospectionResponse::Active(info) = response {
            assert!(matches!(info.aud, Some(TokenAudience::Multiple(_))));
            if let Some(TokenAudience::Multiple(audiences)) = info.aud {
                assert_eq!(audiences.len(), 2);
                assert!(audiences.contains(&"aud1".to_string()));
                assert!(audiences.contains(&"aud2".to_string()));
            }
        } else {
            panic!("Expected active response");
        }
    }

    #[test]
    fn test_serialization() {
        let response = OAuthIntrospectionResponse::active()
            .scope("read")
            .client_id("client123")
            .build();

        let json = response.to_json().unwrap();
        let parsed = OAuthIntrospectionResponse::from_json(&json).unwrap();
        assert_eq!(response, parsed);

        // Test inactive token
        let inactive = OAuthIntrospectionResponse::inactive();
        let json = inactive.to_json().unwrap();
        let parsed = OAuthIntrospectionResponse::from_json(&json).unwrap();
        assert_eq!(inactive, parsed);
    }

    #[test]
    fn test_display() {
        let response = OAuthIntrospectionResponse::active()
            .subject("user123")
            .client_id("client123")
            .build();
        assert!(response.to_string().contains("user123"));
        assert!(response.to_string().contains("client123"));

        let inactive = OAuthIntrospectionResponse::inactive();
        assert_eq!(inactive.to_string(), "Inactive token");
    }
}
