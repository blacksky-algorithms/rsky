use crate::jwk::JwkSet;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const GRANT_AUTHORIZATION_CODE: &str = "authorization_code";
pub const GRANT_REFRESH_TOKEN: &str = "refresh_token";
pub const RESPONSE_TYPE_CODE: &str = "code";
pub const AUTH_METHOD_NONE: &str = "none";
pub const AUTH_METHOD_PRIVATE_KEY_JWT: &str = "private_key_jwt";
pub const APPLICATION_TYPE_WEB: &str = "web";
pub const APPLICATION_TYPE_NATIVE: &str = "native";
pub const CODE_CHALLENGE_METHOD_S256: &str = "S256";
pub const CLIENT_ASSERTION_TYPE_JWT_BEARER: &str =
    "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

pub const SCOPE_ATPROTO: &str = "atproto";
pub const SCOPE_TRANSITION_GENERIC: &str = "transition:generic";
pub const SCOPE_TRANSITION_CHAT_BSKY: &str = "transition:chat.bsky";
pub const SCOPE_TRANSITION_EMAIL: &str = "transition:email";

fn default_application_type() -> String {
    APPLICATION_TYPE_WEB.to_string()
}

fn default_grant_types() -> Vec<String> {
    vec![GRANT_AUTHORIZATION_CODE.to_string()]
}

fn default_response_types() -> Vec<String> {
    vec![RESPONSE_TYPE_CODE.to_string()]
}

/// OAuth client metadata document, fetched from the `client_id` URL
/// (draft-parecki-oauth-client-id-metadata-document).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuthClientMetadata {
    pub client_id: String,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default = "default_grant_types")]
    pub grant_types: Vec<String>,
    #[serde(default = "default_response_types")]
    pub response_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_signing_alg: Option<String>,
    #[serde(default = "default_application_type")]
    pub application_type: String,
    #[serde(default)]
    pub dpop_bound_access_tokens: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks: Option<JwkSet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tos_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contacts: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl OAuthClientMetadata {
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            redirect_uris: Vec::new(),
            grant_types: default_grant_types(),
            response_types: default_response_types(),
            scope: None,
            token_endpoint_auth_method: None,
            token_endpoint_auth_signing_alg: None,
            application_type: default_application_type(),
            dpop_bound_access_tokens: false,
            jwks: None,
            jwks_uri: None,
            client_name: None,
            client_uri: None,
            logo_uri: None,
            tos_uri: None,
            policy_uri: None,
            contacts: Vec::new(),
            extra: Map::new(),
        }
    }

    pub fn auth_method(&self) -> &str {
        self.token_endpoint_auth_method
            .as_deref()
            .unwrap_or(AUTH_METHOD_NONE)
    }

    pub fn is_confidential(&self) -> bool {
        self.auth_method() == AUTH_METHOD_PRIVATE_KEY_JWT
    }

    /// Scopes the client registered for, parsed from the space-separated
    /// `scope` metadata field.
    pub fn allowed_scopes(&self) -> Vec<&str> {
        self.scope
            .as_deref()
            .map(|scope| scope.split_ascii_whitespace().collect())
            .unwrap_or_default()
    }
}

/// The parameters of an authorization request, as pushed via PAR and
/// persisted for the duration of the authorization flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorizationRequestParameters {
    pub client_id: String,
    pub response_type: String,
    pub redirect_uri: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub code_challenge: String,
    pub code_challenge_method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// RFC 7638 thumbprint of the DPoP key used at PAR time, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpop_jkt: Option<String>,
}

/// How the client authenticated, recorded alongside requests and tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ClientAuth {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "private_key_jwt")]
    PrivateKeyJwt {
        alg: String,
        kid: String,
        jkt: String,
    },
}

/// Successful token endpoint response body (RFC 6749 section 5.1 +
/// atproto profile `sub`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub scope: String,
    pub sub: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn client_metadata_defaults() {
        let metadata: OAuthClientMetadata = serde_json::from_value(json!({
            "client_id": "https://app.example.com/client-metadata.json",
        }))
        .unwrap();
        assert_eq!(metadata.grant_types, vec![GRANT_AUTHORIZATION_CODE]);
        assert_eq!(metadata.response_types, vec![RESPONSE_TYPE_CODE]);
        assert_eq!(metadata.application_type, APPLICATION_TYPE_WEB);
        assert_eq!(metadata.auth_method(), AUTH_METHOD_NONE);
        assert!(!metadata.dpop_bound_access_tokens);
        assert!(!metadata.is_confidential());
        assert!(metadata.allowed_scopes().is_empty());
        assert_eq!(
            metadata,
            OAuthClientMetadata::new(metadata.client_id.clone())
        );
    }

    #[test]
    fn client_metadata_full_roundtrip() {
        let json = json!({
            "client_id": "https://app.example.com/client-metadata.json",
            "redirect_uris": ["https://app.example.com/callback"],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "scope": "atproto transition:generic",
            "token_endpoint_auth_method": "private_key_jwt",
            "token_endpoint_auth_signing_alg": "ES256",
            "application_type": "web",
            "dpop_bound_access_tokens": true,
            "jwks": {"keys": []},
            "client_name": "Example",
            "client_uri": "https://app.example.com",
            "logo_uri": "https://app.example.com/logo.png",
            "tos_uri": "https://app.example.com/tos",
            "policy_uri": "https://app.example.com/policy",
            "contacts": ["security@example.com"],
            "custom_field": {"nested": true},
        });
        let metadata: OAuthClientMetadata = serde_json::from_value(json.clone()).unwrap();
        assert!(metadata.is_confidential());
        assert_eq!(metadata.auth_method(), AUTH_METHOD_PRIVATE_KEY_JWT);
        assert_eq!(
            metadata.allowed_scopes(),
            vec![SCOPE_ATPROTO, SCOPE_TRANSITION_GENERIC]
        );
        assert_eq!(serde_json::to_value(&metadata).unwrap(), json);
    }

    #[test]
    fn client_auth_serde() {
        let none = ClientAuth::None;
        assert_eq!(
            serde_json::to_value(&none).unwrap(),
            json!({"method": "none"})
        );
        let jwt = ClientAuth::PrivateKeyJwt {
            alg: "ES256".to_string(),
            kid: "key-1".to_string(),
            jkt: "thumb".to_string(),
        };
        let value = serde_json::to_value(&jwt).unwrap();
        assert_eq!(value["method"], "private_key_jwt");
        let parsed: ClientAuth = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, jwt);
    }

    #[test]
    fn token_response_serde() {
        let response = TokenResponse {
            access_token: "at".to_string(),
            token_type: "DPoP".to_string(),
            expires_in: 3600,
            refresh_token: Some("rt".to_string()),
            scope: "atproto".to_string(),
            sub: "did:plc:alice".to_string(),
        };
        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["token_type"], "DPoP");
        let parsed: TokenResponse = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, response);
    }
}
