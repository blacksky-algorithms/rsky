use crate::oauth_provider::constants::{CODE_INNER_LENGTH, CODE_PREFIX};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

const CODE_LENGTH: usize = CODE_PREFIX.len() + CODE_INNER_LENGTH; //hex encoding

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
            .take(CODE_INNER_LENGTH)
            .map(char::from)
            .collect();
        let val = CODE_PREFIX.to_string() + token.as_str();
        Code::new(val).unwrap()
    }
}

impl Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code() {
        let code = Code::new("tok-dwadwdaddwadwdad").unwrap();
        assert_eq!(code.into_inner(), "tok-dwadwdaddwadwdad");
        let code = Code::generate();
        let val = code.into_inner();
        Code::new(val).unwrap();

        let invalid_format_code = Code::new("aaaadwadwdaddwadwdad").unwrap_err();
        assert_eq!(invalid_format_code, CodeError::InvalidFormat);

        let invalid_length = Code::new("tok-dwadwda").unwrap_err();
        assert_eq!(invalid_length, CodeError::InvalidLength);
    }
}
