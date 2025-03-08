use crate::oauth_provider::constants::{REFRESH_TOKEN_BYTES_LENGTH, REFRESH_TOKEN_PREFIX};
use serde::{Deserialize, Serialize};

pub const REFRESH_TOKEN_LENGTH: usize = REFRESH_TOKEN_PREFIX.len() + REFRESH_TOKEN_BYTES_LENGTH * 2; // hex encoding

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshToken(String);

impl RefreshToken {
    pub fn new(val: impl Into<String>) -> Self {
        RefreshToken(val.into())
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }
}

pub enum RefreshTokenError {
    InvalidLength,
    InvalidFormat(String),
}

pub fn is_refresh_token(data: &str) -> bool {
    let prefix = &data.to_string()[..4];
    prefix == REFRESH_TOKEN_PREFIX
}

pub async fn generate_refresh_token() -> RefreshToken {
    let val = "";
    RefreshToken::new(val.to_string())
}
