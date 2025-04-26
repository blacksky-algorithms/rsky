use serde::{Deserialize, Serialize};
use std::fmt;

use crate::jwk::JwtToken;
use crate::oauth_types::{
    OAuthAuthorizationDetails, OAuthClientId, OAuthClientIdError, OAuthCodeChallengeMethod,
    OAuthRedirectUri, OAuthResponseType, OAuthScope, OAuthScopeError, RedirectUriError,
};

/// Parameters for an OAuth authorization request.
///
/// This represents all the possible parameters that can be included
/// in an authorization request, as defined in OAuth 2.1 and OpenID Connect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthAuthorizationRequestParameters {
    /// Client identifier
    pub client_id: OAuthClientId,

    /// Optional state value for CSRF protection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,

    /// Redirect URI after authorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<OAuthRedirectUri>,

    /// OAuth scopes requested
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<OAuthScope>,

    /// Response type
    pub response_type: OAuthResponseType,

    /// PKCE code challenge (required)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_challenge: Option<String>,

    /// PKCE code challenge method
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_challenge_method: Option<OAuthCodeChallengeMethod>,

    /// DPoP JWK thumbprint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpop_jkt: Option<String>,

    /// Response mode for auth response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mode: Option<ResponseMode>,

    /// Nonce for replay prevention
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,

    /// Max authentication age in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age: Option<u32>,

    /// Claims being requested
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claims: Option<ClaimsRequest>,

    /// Login hint (e.g. email or username)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_hint: Option<String>,

    /// Preferred UI languages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_locales: Option<String>,

    /// ID Token from previous session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token_hint: Option<JwtToken>,

    /// UI display type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,

    /// Prompt options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Prompt>,

    /// Authorization details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_details: Option<OAuthAuthorizationDetails>,
}

/// Display types for the authorization UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Display {
    /// Full page display
    Page,
    /// Popup window display
    Popup,
    /// Touch-optimized display
    Touch,
    /// WAP mobile display
    Wap,
}

/// Response mode for the authorization response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    /// Return in query parameters
    Query,
    /// Return in fragment
    Fragment,
    /// Return in form POST
    FormPost,
}

/// Prompt options for login/consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Prompt {
    /// No interactive prompt
    None,
    /// Force login prompt
    Login,
    /// Force consent prompt
    Consent,
    /// Force account selection
    SelectAccount,
}

/// Claims request structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimsRequest {
    #[serde(flatten)]
    pub claims: std::collections::HashMap<String, ClaimProperties>,
}

/// Properties for a requested claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimProperties {
    /// Whether claim is essential
    #[serde(skip_serializing_if = "Option::is_none")]
    pub essential: Option<bool>,
    /// Expected claim value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// List of acceptable values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<serde_json::Value>>,
}

impl OAuthAuthorizationRequestParameters {
    /// Create new authorization request parameters with required fields.
    pub fn new(
        client_id: OAuthClientId,
        response_type: OAuthResponseType,
        redirect_uri: Option<OAuthRedirectUri>,
        scope: Option<OAuthScope>,
        state: Option<String>,
    ) -> Result<Self, RequestError> {
        // Validate state token if provided
        if let Some(state) = &state {
            if state.is_empty() {
                return Err(RequestError::InvalidState);
            }
        }

        Ok(Self {
            client_id,
            state,
            redirect_uri,
            scope,
            response_type,
            code_challenge: None,
            code_challenge_method: None,
            dpop_jkt: None,
            response_mode: None,
            nonce: None,
            max_age: None,
            claims: None,
            login_hint: None,
            ui_locales: None,
            id_token_hint: None,
            display: None,
            prompt: None,
            authorization_details: None,
        })
    }

    /// Set the PKCE code challenge.
    pub fn with_code_challenge(
        mut self,
        challenge: impl Into<String>,
        method: OAuthCodeChallengeMethod,
    ) -> Self {
        self.code_challenge = Some(challenge.into());
        self.code_challenge_method = Some(method);
        self
    }

    /// Set the DPoP JWK thumbprint.
    pub fn with_dpop_jkt(mut self, jkt: impl Into<String>) -> Self {
        self.dpop_jkt = Some(jkt.into());
        self
    }

    /// Set the response mode.
    pub fn with_response_mode(mut self, mode: ResponseMode) -> Self {
        self.response_mode = Some(mode);
        self
    }

    /// Set the nonce.
    pub fn with_nonce(mut self, nonce: impl Into<String>) -> Self {
        self.nonce = Some(nonce.into());
        self
    }
}

impl fmt::Display for OAuthAuthorizationRequestParameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AuthorizationRequest(client={}, type={}",
            self.client_id, self.response_type
        )?;

        if let Some(scope) = &self.scope {
            write!(f, ", scope={}", scope)?;
        }

        write!(f, ")")
    }
}

/// Errors that can occur with authorization requests.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("Missing client_id parameter")]
    MissingClientId,

    #[error("Missing response_type parameter")]
    MissingResponseType,

    #[error("Invalid state token")]
    InvalidState,

    #[error("Client ID error: {0}")]
    ClientId(#[from] OAuthClientIdError),

    #[error("Scope error: {0}")]
    Scope(#[from] OAuthScopeError),

    #[error("Redirect URI error: {0}")]
    RedirectUri(#[from] RedirectUriError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client_id() -> OAuthClientId {
        OAuthClientId::new("test_client").unwrap()
    }

    #[test]
    fn test_new_parameters() {
        let params = OAuthAuthorizationRequestParameters::new(
            test_client_id(),
            OAuthResponseType::Code,
            None,
            Some(OAuthScope::new("read write").unwrap()),
            Some("state123".to_string()),
        )
        .unwrap()
        .with_nonce("nonce123")
        .with_response_mode(ResponseMode::Query);

        let json = serde_json::to_string(&params).unwrap();
        let deserialized: OAuthAuthorizationRequestParameters =
            serde_json::from_str(&json).unwrap();

        assert_eq!(params, deserialized);
        assert_eq!(deserialized.nonce.unwrap(), "nonce123");
        assert_eq!(deserialized.response_mode.unwrap(), ResponseMode::Query);
    }

    #[test]
    fn test_claims_request() {
        use serde_json::json;

        let mut claims = std::collections::HashMap::new();
        claims.insert(
            "email".to_string(),
            ClaimProperties {
                essential: Some(true),
                value: Some(json!("test@example.com")),
                values: None,
            },
        );

        let params = OAuthAuthorizationRequestParameters::new(
            test_client_id(),
            OAuthResponseType::Code,
            None,
            None,
            None,
        )
        .unwrap();

        let json = serde_json::to_string(&params).unwrap();
        assert!(!json.contains("claims")); // Claims not included when None

        // Test full claims serialization
        let mut params_with_claims = params;
        params_with_claims.claims = Some(ClaimsRequest { claims });

        let json = serde_json::to_string(&params_with_claims).unwrap();
        assert!(json.contains("claims"));
        assert!(json.contains("essential"));
        assert!(json.contains("test@example.com"));
    }

    #[test]
    fn test_display() {
        let params = OAuthAuthorizationRequestParameters::new(
            test_client_id(),
            OAuthResponseType::Code,
            None,
            Some(OAuthScope::new("read write").unwrap()),
            None,
        )
        .unwrap();

        assert_eq!(
            params.to_string(),
            "AuthorizationRequest(client=test_client, type=code, scope=read write)"
        );
    }

    #[test]
    fn test_optional_parameters() {
        let mut params = OAuthAuthorizationRequestParameters::new(
            test_client_id(),
            OAuthResponseType::Code,
            None,
            None,
            None,
        )
        .unwrap();

        params = params
            .with_code_challenge("challenge", OAuthCodeChallengeMethod::S256)
            .with_dpop_jkt("jkt123")
            .with_response_mode(ResponseMode::Query)
            .with_nonce("nonce123");

        assert_eq!(params.code_challenge.unwrap(), "challenge");
        assert_eq!(
            params.code_challenge_method.unwrap(),
            OAuthCodeChallengeMethod::S256
        );
        assert_eq!(params.dpop_jkt.unwrap(), "jkt123");
        assert_eq!(params.response_mode.unwrap(), ResponseMode::Query);
        assert_eq!(params.nonce.unwrap(), "nonce123");
    }

    #[test]
    fn test_serialization() {
        let params = OAuthAuthorizationRequestParameters::new(
            test_client_id(),
            OAuthResponseType::Code,
            None,
            Some(OAuthScope::new("read write").unwrap()),
            Some("state123".to_string()),
        )
        .unwrap();

        assert_eq!(params.client_id.as_ref(), "test_client");
        assert_eq!(params.response_type, OAuthResponseType::Code);
        assert_eq!(params.scope.as_ref().unwrap().as_ref(), "read write");
        assert_eq!(params.state.as_ref().unwrap(), "state123");
    }
}
