use crate::oauth_provider::constants::{CODE_BYTES_LENGTH, CODE_PREFIX};
use crate::oauth_provider::errors::OAuthError;
use serde::{Deserialize, Serialize};

const CODE_LENGTH: u64 = CODE_PREFIX.length() + CODE_BYTES_LENGTH * 2; //hex encoding

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Code(String);

impl Code {
    pub fn new(val: impl Into<String>) -> Result<Self, OAuthError> {
        Ok(Self(val.into()))
    }
}

pub fn parse(input: String) -> Result<String, OAuthError> {
    Ok("code".to_string())
}

pub async fn generate_code() -> Code {
    unimplemented!()
}
