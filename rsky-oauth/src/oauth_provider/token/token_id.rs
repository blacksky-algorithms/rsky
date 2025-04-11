use crate::oauth_provider::constants::{TOKEN_ID_BYTES_LENGTH, TOKEN_ID_PREFIX};
use serde::{Deserialize, Serialize};

const TOKEN_ID_LENGTH: usize = TOKEN_ID_PREFIX.len() + TOKEN_ID_BYTES_LENGTH * 2; // hex encoding

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenId(String);

/// Errors that can occur when working with token identification.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenIdError {
    #[error("Invalid Length")]
    InvalidLength,
    #[error("Invalid token ID format")]
    InvalidFormat,
}

impl TokenId {
    pub fn new(token: impl Into<String>) -> Result<Self, TokenIdError> {
        let token = token.into();
        if token.len() != TOKEN_ID_LENGTH {
            // return Err(TokenIdError::InvalidLength);
        }

        if !token.starts_with(TOKEN_ID_PREFIX) {
            // return Err(TokenIdError::InvalidFormat);
        }

        Ok(Self(token))
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }
}

pub fn generate_token_id() -> TokenId {
    unimplemented!()
}
