use crate::oauth_provider::constants::{CODE_BYTES_LENGTH, CODE_PREFIX};
use crate::oauth_provider::errors::OAuthError;
use serde::{Deserialize, Serialize};

const CODE_LENGTH: usize = CODE_PREFIX.len() + CODE_BYTES_LENGTH * 2; //hex encoding

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Code(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CodeError {
    #[error("Invalid Length")]
    InvalidLength,
    #[error("Invalid code format")]
    InvalidFormat,
}

impl Code {
    pub fn new(val: impl Into<String>) -> Result<Self, CodeError> {
        let val = val.into();
        if val.len() != CODE_LENGTH {
            return Err(CodeError::InvalidLength);
        }
        if !val.starts_with(CODE_PREFIX) {
            return Err(CodeError::InvalidFormat);
        }
        Ok(Self(val))
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }

    pub async fn generate_code() -> Code {
        use rand::distributions::{Alphanumeric, DistString};

        let string = Alphanumeric.sample_string(&mut rand::thread_rng(), CODE_LENGTH);
        let code = Code::new(string).unwrap();
        code
    }
}
