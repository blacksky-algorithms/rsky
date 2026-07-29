use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OAuthError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    InvalidToken(String),
    #[error("{0}")]
    InvalidDpopProof(String),
    #[error("{0}")]
    UseDpopNonce(String),
    #[error("{0}")]
    InvalidClient(String),
    #[error("{0}")]
    InvalidGrant(String),
    #[error("{0}")]
    ServerError(String),
}

impl OAuthError {
    pub fn use_dpop_nonce() -> Self {
        Self::UseDpopNonce("Authorization server requires nonce in DPoP proof".to_string())
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::InvalidRequest(_) => "invalid_request",
            Self::InvalidToken(_) => "invalid_token",
            Self::InvalidDpopProof(_) => "invalid_dpop_proof",
            Self::UseDpopNonce(_) => "use_dpop_nonce",
            Self::InvalidClient(_) => "invalid_client",
            Self::InvalidGrant(_) => "invalid_grant",
            Self::ServerError(_) => "server_error",
        }
    }

    pub fn error_description(&self) -> &str {
        match self {
            Self::InvalidRequest(description)
            | Self::InvalidToken(description)
            | Self::InvalidDpopProof(description)
            | Self::UseDpopNonce(description)
            | Self::InvalidClient(description)
            | Self::InvalidGrant(description)
            | Self::ServerError(description) => description,
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::InvalidToken(_) | Self::InvalidDpopProof(_) | Self::InvalidClient(_) => 401,
            Self::ServerError(_) => 500,
            _ => 400,
        }
    }

    /// True when the HTTP response for this error must carry a fresh
    /// DPoP-Nonce header.
    pub fn requires_dpop_nonce(&self) -> bool {
        matches!(self, Self::UseDpopNonce(_))
    }

    pub fn to_json(&self) -> Value {
        json!({
            "error": self.error_code(),
            "error_description": self.error_description(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_and_statuses() {
        let cases: [(OAuthError, &str, &str, u16); 7] = [
            (
                OAuthError::InvalidRequest("a".into()),
                "invalid_request",
                "a",
                400,
            ),
            (
                OAuthError::InvalidToken("b".into()),
                "invalid_token",
                "b",
                401,
            ),
            (
                OAuthError::InvalidDpopProof("c".into()),
                "invalid_dpop_proof",
                "c",
                401,
            ),
            (
                OAuthError::UseDpopNonce("d".into()),
                "use_dpop_nonce",
                "d",
                400,
            ),
            (
                OAuthError::InvalidClient("e".into()),
                "invalid_client",
                "e",
                401,
            ),
            (
                OAuthError::InvalidGrant("f".into()),
                "invalid_grant",
                "f",
                400,
            ),
            (
                OAuthError::ServerError("g".into()),
                "server_error",
                "g",
                500,
            ),
        ];
        for (error, code, description, status) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.error_description(), description);
            assert_eq!(error.status(), status);
        }
    }

    #[test]
    fn description_and_display_match() {
        let error = OAuthError::InvalidDpopProof("DPoP \"htm\" mismatch".to_string());
        assert_eq!(error.error_description(), "DPoP \"htm\" mismatch");
        assert_eq!(error.to_string(), "DPoP \"htm\" mismatch");
    }

    #[test]
    fn use_dpop_nonce_constructor_and_json() {
        let error = OAuthError::use_dpop_nonce();
        assert!(error.requires_dpop_nonce());
        assert!(!OAuthError::InvalidGrant("x".into()).requires_dpop_nonce());
        assert_eq!(
            error.to_json(),
            serde_json::json!({
                "error": "use_dpop_nonce",
                "error_description": "Authorization server requires nonce in DPoP proof",
            })
        );
    }
}
