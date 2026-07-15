use crate::error::OAuthError;
use crate::types::{AuthorizationRequestParameters, ClientAuth};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TOKEN_ID_PREFIX: &str = "tok-";
pub const TOKEN_ID_BYTES_LENGTH: usize = 16;
pub const REFRESH_TOKEN_PREFIX: &str = "ref-";
pub const REFRESH_TOKEN_BYTES_LENGTH: usize = 32;

/// Access token lifetime, in seconds (60 minutes).
pub const TOKEN_MAX_AGE: u64 = 3600;
/// Public client total session lifetime (2 weeks).
pub const PUBLIC_CLIENT_SESSION_LIFETIME: u64 = 2 * 7 * 24 * 3600;
/// Public client refresh inactivity lifetime (2 weeks).
pub const PUBLIC_CLIENT_REFRESH_LIFETIME: u64 = PUBLIC_CLIENT_SESSION_LIFETIME;
/// Confidential client total session lifetime (2 years).
pub const CONFIDENTIAL_CLIENT_SESSION_LIFETIME: u64 = 2 * 365 * 24 * 3600;
/// Confidential client refresh inactivity lifetime (3 months).
pub const CONFIDENTIAL_CLIENT_REFRESH_LIFETIME: u64 = 90 * 24 * 3600;

fn random_hex_id(prefix: &str, bytes: usize) -> String {
    format!(
        "{prefix}{}",
        hex::encode(rsky_crypto::utils::random_bytes(bytes))
    )
}

pub fn generate_token_id() -> String {
    random_hex_id(TOKEN_ID_PREFIX, TOKEN_ID_BYTES_LENGTH)
}

pub fn generate_refresh_token() -> String {
    random_hex_id(REFRESH_TOKEN_PREFIX, REFRESH_TOKEN_BYTES_LENGTH)
}

pub fn is_token_id(value: &str) -> bool {
    value.starts_with(TOKEN_ID_PREFIX)
        && value.len() == TOKEN_ID_PREFIX.len() + TOKEN_ID_BYTES_LENGTH * 2
}

pub fn is_refresh_token(value: &str) -> bool {
    value.starts_with(REFRESH_TOKEN_PREFIX)
        && value.len() == REFRESH_TOKEN_PREFIX.len() + REFRESH_TOKEN_BYTES_LENGTH * 2
}

/// RFC 7636 section 4.6: base64url(SHA-256(verifier)) must equal the
/// stored S256 code challenge.
pub fn verify_code_challenge(verifier: &str, challenge: &str) -> Result<(), OAuthError> {
    if verifier.len() < 43 || verifier.len() > 128 {
        return Err(OAuthError::InvalidGrant(
            "invalid code_verifier length".to_string(),
        ));
    }
    let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    if computed == challenge {
        Ok(())
    } else {
        Err(OAuthError::InvalidGrant(
            "invalid code_verifier".to_string(),
        ))
    }
}

/// A stored token, mirroring the upstream `token` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenData {
    /// Unix seconds; fixed at issuance, bounds the total session lifetime.
    pub created_at: u64,
    /// Unix seconds; advanced on each rotation (sliding inactivity window).
    pub updated_at: u64,
    /// Unix seconds; access token expiry.
    pub expires_at: u64,
    pub client_id: String,
    pub client_auth: ClientAuth,
    pub device_id: Option<String>,
    pub did: String,
    pub parameters: AuthorizationRequestParameters,
    pub code: Option<String>,
}

impl TokenData {
    fn is_confidential(&self) -> bool {
        !matches!(self.client_auth, ClientAuth::None)
    }

    /// Enforces the refresh-grant lifetimes: total session age from
    /// `created_at` and inactivity age from `updated_at`.
    pub fn validate_refresh_lifetimes(&self, now: u64) -> Result<(), OAuthError> {
        let (session_lifetime, refresh_lifetime) = if self.is_confidential() {
            (
                CONFIDENTIAL_CLIENT_SESSION_LIFETIME,
                CONFIDENTIAL_CLIENT_REFRESH_LIFETIME,
            )
        } else {
            (
                PUBLIC_CLIENT_SESSION_LIFETIME,
                PUBLIC_CLIENT_REFRESH_LIFETIME,
            )
        };
        if now.saturating_sub(self.created_at) > session_lifetime {
            return Err(OAuthError::InvalidGrant("session expired".to_string()));
        }
        if now.saturating_sub(self.updated_at) > refresh_lifetime {
            return Err(OAuthError::InvalidGrant(
                "refresh token expired".to_string(),
            ));
        }
        Ok(())
    }
}

/// A stored token together with its identifiers.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenInfo {
    pub token_id: String,
    pub data: TokenData,
    pub current_refresh_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CODE_CHALLENGE_METHOD_S256;

    const NOW: u64 = 1_700_000_000;

    fn token_data(client_auth: ClientAuth) -> TokenData {
        TokenData {
            created_at: NOW,
            updated_at: NOW,
            expires_at: NOW + TOKEN_MAX_AGE,
            client_id: "https://app.example.com/client".to_string(),
            client_auth,
            device_id: None,
            did: "did:plc:alice".to_string(),
            parameters: AuthorizationRequestParameters {
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
            },
            code: None,
        }
    }

    #[test]
    fn id_formats() {
        let token_id = generate_token_id();
        assert!(is_token_id(&token_id));
        assert_eq!(token_id.len(), 4 + 32);
        assert!(!is_token_id("tok-short"));
        assert!(!is_token_id(&generate_refresh_token()));

        let refresh = generate_refresh_token();
        assert!(is_refresh_token(&refresh));
        assert_eq!(refresh.len(), 4 + 64);
        assert!(!is_refresh_token(&token_id));
        assert_ne!(generate_token_id(), generate_token_id());
    }

    #[test]
    fn pkce_s256_verification() {
        // RFC 7636 appendix B test vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        verify_code_challenge(verifier, challenge).unwrap();
        assert_eq!(
            verify_code_challenge(verifier, "wrong").unwrap_err(),
            OAuthError::InvalidGrant("invalid code_verifier".to_string())
        );
        assert_eq!(
            verify_code_challenge("too-short", challenge).unwrap_err(),
            OAuthError::InvalidGrant("invalid code_verifier length".to_string())
        );
        assert!(verify_code_challenge(&"a".repeat(129), challenge).is_err());
    }

    #[test]
    fn refresh_lifetimes_public_client() {
        let mut data = token_data(ClientAuth::None);
        data.validate_refresh_lifetimes(NOW).unwrap();
        data.validate_refresh_lifetimes(NOW + PUBLIC_CLIENT_REFRESH_LIFETIME)
            .unwrap();
        assert_eq!(
            data.validate_refresh_lifetimes(NOW + PUBLIC_CLIENT_SESSION_LIFETIME + 1)
                .unwrap_err(),
            OAuthError::InvalidGrant("session expired".to_string())
        );
        // an old updated_at trips the inactivity check while the session
        // is still within its total lifetime
        data.updated_at = NOW - 1000;
        assert_eq!(
            data.validate_refresh_lifetimes(NOW + PUBLIC_CLIENT_REFRESH_LIFETIME)
                .unwrap_err(),
            OAuthError::InvalidGrant("refresh token expired".to_string())
        );
    }

    #[test]
    fn refresh_lifetimes_confidential_client() {
        let data = token_data(ClientAuth::PrivateKeyJwt {
            alg: "ES256".to_string(),
            kid: "key-1".to_string(),
            jkt: "thumb".to_string(),
        });
        data.validate_refresh_lifetimes(NOW + PUBLIC_CLIENT_SESSION_LIFETIME + 1)
            .unwrap();
        assert_eq!(
            data.validate_refresh_lifetimes(NOW + CONFIDENTIAL_CLIENT_REFRESH_LIFETIME + 1)
                .unwrap_err(),
            OAuthError::InvalidGrant("refresh token expired".to_string())
        );
        let mut fresh = data.clone();
        fresh.updated_at = NOW + CONFIDENTIAL_CLIENT_SESSION_LIFETIME;
        assert_eq!(
            fresh
                .validate_refresh_lifetimes(NOW + CONFIDENTIAL_CLIENT_SESSION_LIFETIME + 1)
                .unwrap_err(),
            OAuthError::InvalidGrant("session expired".to_string())
        );
    }

    #[test]
    fn token_data_serde_roundtrip() {
        let data = token_data(ClientAuth::None);
        let json = serde_json::to_string(&data).unwrap();
        let parsed: TokenData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, data);
    }
}
