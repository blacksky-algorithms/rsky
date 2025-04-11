use crate::oauth_provider::request::request_id::RequestId;
use std::fmt;
use std::fmt::{Display, Formatter};

const ERR_JWKS_NO_MATCHING_KEY: &str = "ERR_JWKS_NO_MATCHING_KEY";
const ERR_JWK_INVALID: &str = "ERR_JWK_INVALID";
const ERR_JWK_NOT_FOUND: &str = "ERR_JWK_NOT_FOUND";
const ERR_JWT_INVALID: &str = "ERR_JWT_INVALID";
const ERR_JWT_CREATE: &str = "ERR_JWT_CREATE";
const ERR_JWT_VERIFY: &str = "ERR_JWT_VERIFY";

#[derive(Debug)]
pub enum JwkError {
    JwtCreateError(String),
    JwtVerifyError(String),
    Other(String),
}

impl Display for JwkError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            JwkError::JwtCreateError(res) => {
                write!(f, "JwkCreateError: {}", res)
            }
            JwkError::JwtVerifyError(res) => {
                write!(f, "JwkVerifyError: {}", res)
            }
            JwkError::Other(res) => {
                write!(f, "JwkOtherError: {}", res)
            }
        }
    }
}
