use crate::oauth_provider::constants::{CODE_BYTES_LENGTH, CODE_PREFIX};
use rand::Rng;
use serde::{Deserialize, Serialize};

const CODE_LENGTH: usize = CODE_PREFIX.len() + CODE_BYTES_LENGTH; //hex encoding

#[derive(Debug, Deserialize, Serialize, Eq, Clone, PartialEq)]
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

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn generate() -> Code {
        use rand::distr::Alphanumeric;
        let token: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(CODE_BYTES_LENGTH)
            .map(char::from)
            .collect();
        let val = CODE_PREFIX.to_string() + token.as_str();
        Code::new(val).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code() {
        // let token_id = TokenId::new("tok-dwadwdaddwadwdad").unwrap();
        // assert_eq!(token_id.into_inner(), "tok-dwadwdaddwadwdad");
        // let token_id = TokenId::generate();
        // let val = token_id.into_inner();
        // TokenId::new(val).unwrap();
        //
        // let invalid_format_token_id = TokenId::new("aaaadwadwdaddwadwdad").unwrap_err();
        // assert_eq!(invalid_format_token_id, TokenIdError::InvalidFormat);
        //
        // let invalid_length = TokenId::new("tok-dwadwda").unwrap_err();
        // assert_eq!(invalid_length, TokenIdError::InvalidLength);
    }
}
