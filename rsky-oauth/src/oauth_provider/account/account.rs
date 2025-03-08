use crate::oauth_provider::oidc::sub::Sub;

#[derive(Clone)]
pub struct Account {
    ///Account ID
    pub sub: Sub,
    /// Resource server URL
    pub aud: Vec<String>,

    /// OIDC inspired
    pub preferred_username: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub picture: Option<String>,
    pub name: Option<String>,
}
