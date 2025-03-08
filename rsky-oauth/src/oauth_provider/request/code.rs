use crate::oauth_provider::errors::OAuthError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Code {}

pub fn parse(input: String) -> Result<String, OAuthError> {
    Ok("code".to_string())
}
