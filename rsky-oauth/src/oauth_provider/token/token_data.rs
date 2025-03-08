use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::request::code::Code;
use crate::oauth_types::{OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TokenData {
    pub created_at: u64,
    pub updated_at: u64,
    pub expires_at: u64,
    pub client_id: String,
    pub client_auth: ClientAuth,
    pub device_id: Option<DeviceId>,
    pub sub: Sub,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub details: Option<OAuthAuthorizationDetails>,
    pub code: Option<Code>,
}
