use jsonwebtoken::Header;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct VerifyOptions {
    pub audience: Option<String>,
    /** in seconds */
    pub clock_tolerance: Option<u64>,
    pub issuer: Option<String>,
    /** in seconds */
    pub max_token_age: Option<u64>,
    pub subject: Option<String>,
    pub typ: Option<String>,
    pub current_date: Option<u64>,
    pub required_claimed: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct VerifyResult {
    pub payload: String,
    pub protected_header: Header,
}
