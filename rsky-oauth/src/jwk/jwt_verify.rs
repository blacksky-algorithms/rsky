use crate::jwk::{JwtHeader, JwtPayload};
use crate::oauth_types::OAuthIssuerIdentifier;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct VerifyOptions {
    pub audience: Option<String>,
    /** in seconds */
    pub clock_tolerance: Option<i64>,
    pub issuer: Option<OAuthIssuerIdentifier>,
    /** in seconds */
    pub max_token_age: Option<i64>,
    pub subject: Option<String>,
    pub typ: Option<String>,
    pub current_date: Option<i64>,
    pub required_claims: Vec<String>,
}

#[derive(Serialize, Eq, PartialEq, Deserialize, Debug)]
pub struct VerifyResult {
    pub payload: JwtPayload,
    pub protected_header: JwtHeader,
}

#[derive(Serialize, Eq, PartialEq, Deserialize, Debug)]
pub struct UnsecuredResult {
    pub payload: JwtPayload,
    pub header: JwtHeader,
}
