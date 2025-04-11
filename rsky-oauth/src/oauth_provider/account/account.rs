use crate::jwk::Audience;
use crate::oauth_provider::oidc::sub::Sub;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    ///Account ID
    pub sub: Sub,
    /// Resource server URL
    pub aud: Audience,

    /// OIDC inspired
    pub preferred_username: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub picture: Option<String>,
    pub name: Option<String>,
}
