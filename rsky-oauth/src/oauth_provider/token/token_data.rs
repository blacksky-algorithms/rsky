use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::request::code::Code;
use crate::oauth_types::{
    OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters, OAuthClientId,
};
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenData {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub client_id: OAuthClientId,
    pub client_auth: ClientAuth,
    pub device_id: Option<DeviceId>,
    pub sub: Sub,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub details: Option<OAuthAuthorizationDetails>,
    pub code: Option<Code>,
}
