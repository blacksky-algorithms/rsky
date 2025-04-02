use crate::oauth_provider::constants::{TOKEN_ID_BYTES_LENGTH, TOKEN_ID_PREFIX};
use serde::{Deserialize, Serialize};

const TOKEN_ID_LENGTH: usize = TOKEN_ID_PREFIX.len() + TOKEN_ID_BYTES_LENGTH * 2; // hex encoding

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignedTokenPayload(String);

/// Errors that can occur when working with token identification.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignedTokenPayloadError {
    #[error("Invalid Length")]
    InvalidLength,
    #[error("Invalid token ID format")]
    InvalidFormat,
}

impl SignedTokenPayload {
    pub fn new(token: impl Into<String>) -> Result<Self, SignedTokenPayloadError> {
        let token = token.into();
        if token.len() != TOKEN_ID_LENGTH {
            return Err(SignedTokenPayloadError::InvalidLength);
        }

        if !token.starts_with(TOKEN_ID_PREFIX) {
            return Err(SignedTokenPayloadError::InvalidFormat);
        }

        Ok(Self(token))
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }
}
