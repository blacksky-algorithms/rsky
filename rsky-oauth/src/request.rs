use crate::error::OAuthError;
use crate::types::{AuthorizationRequestParameters, ClientAuth};
use serde::{Deserialize, Serialize};

pub const REQUEST_ID_PREFIX: &str = "req-";
pub const REQUEST_ID_BYTES_LENGTH: usize = 16;
pub const CODE_PREFIX: &str = "cod-";
pub const CODE_BYTES_LENGTH: usize = 32;
pub const REQUEST_URI_PREFIX: &str = "urn:ietf:params:oauth:request_uri:";

/// Lifetime of a pushed authorization request, in seconds.
pub const PAR_EXPIRES_IN: u64 = 300;
/// Sliding inactivity window during the authorization flow, also the
/// authorization code exchange window, in seconds.
pub const AUTHORIZATION_INACTIVITY_TIMEOUT: u64 = 300;

fn random_hex_id(prefix: &str, bytes: usize) -> String {
    format!(
        "{prefix}{}",
        hex::encode(rsky_crypto::utils::random_bytes(bytes))
    )
}

pub fn generate_request_id() -> String {
    random_hex_id(REQUEST_ID_PREFIX, REQUEST_ID_BYTES_LENGTH)
}

pub fn generate_code() -> String {
    random_hex_id(CODE_PREFIX, CODE_BYTES_LENGTH)
}

pub fn request_uri_from_id(request_id: &str) -> String {
    format!("{REQUEST_URI_PREFIX}{request_id}")
}

pub fn request_id_from_uri(request_uri: &str) -> Result<&str, OAuthError> {
    match request_uri.strip_prefix(REQUEST_URI_PREFIX) {
        Some(request_id)
            if request_id.starts_with(REQUEST_ID_PREFIX)
                && request_id.len() == REQUEST_ID_PREFIX.len() + REQUEST_ID_BYTES_LENGTH * 2 =>
        {
            Ok(request_id)
        }
        _ => Err(OAuthError::InvalidRequest(
            "invalid request_uri".to_string(),
        )),
    }
}

pub fn is_code(value: &str) -> bool {
    value.starts_with(CODE_PREFIX) && value.len() == CODE_PREFIX.len() + CODE_BYTES_LENGTH * 2
}

/// A stored authorization request, mirroring the upstream
/// `authorization_request` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestData {
    pub client_id: String,
    pub client_auth: ClientAuth,
    pub parameters: AuthorizationRequestParameters,
    /// Unix seconds.
    pub expires_at: u64,
    pub device_id: Option<String>,
    pub did: Option<String>,
    pub code: Option<String>,
}

impl RequestData {
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at <= now
    }

    pub fn is_authorized(&self) -> bool {
        self.did.is_some() || self.code.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CODE_CHALLENGE_METHOD_S256;

    fn parameters() -> AuthorizationRequestParameters {
        AuthorizationRequestParameters {
            client_id: "https://app.example.com/client".to_string(),
            response_type: "code".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "atproto".to_string(),
            state: None,
            code_challenge: "challenge".to_string(),
            code_challenge_method: CODE_CHALLENGE_METHOD_S256.to_string(),
            login_hint: None,
            prompt: None,
            dpop_jkt: None,
        }
    }

    #[test]
    fn id_generation_and_request_uri_roundtrip() {
        let request_id = generate_request_id();
        assert!(request_id.starts_with(REQUEST_ID_PREFIX));
        assert_eq!(request_id.len(), 4 + 32);
        assert_ne!(request_id, generate_request_id());
        let uri = request_uri_from_id(&request_id);
        assert_eq!(request_id_from_uri(&uri).unwrap(), request_id);
    }

    #[test]
    fn code_generation() {
        let code = generate_code();
        assert!(code.starts_with(CODE_PREFIX));
        assert_eq!(code.len(), 4 + 64);
        assert!(is_code(&code));
        assert!(!is_code("cod-short"));
        assert!(!is_code(
            "tok-0000000000000000000000000000000000000000000000000000000000000000"
        ));
    }

    #[test]
    fn invalid_request_uris_rejected() {
        assert!(request_id_from_uri("urn:example:nope").is_err());
        assert!(request_id_from_uri(&format!("{REQUEST_URI_PREFIX}bogus")).is_err());
        assert!(request_id_from_uri(&format!("{REQUEST_URI_PREFIX}req-short")).is_err());
    }

    #[test]
    fn request_data_lifecycle_flags() {
        let mut data = RequestData {
            client_id: "https://app.example.com/client".to_string(),
            client_auth: ClientAuth::None,
            parameters: parameters(),
            expires_at: 1000,
            device_id: None,
            did: None,
            code: None,
        };
        assert!(!data.is_authorized());
        assert!(!data.is_expired(999));
        assert!(data.is_expired(1000));
        data.did = Some("did:plc:alice".to_string());
        assert!(data.is_authorized());
        data.did = None;
        data.code = Some(generate_code());
        assert!(data.is_authorized());
        let json = serde_json::to_string(&data).unwrap();
        let parsed: RequestData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, data);
    }
}
