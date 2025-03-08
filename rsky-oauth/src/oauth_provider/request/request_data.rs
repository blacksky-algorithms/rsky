use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::request::code::Code;
use crate::oauth_types::OAuthAuthorizationRequestParameters;

pub enum RequestDataEnum {
    UNAUTHORIZED(RequestData),
    AUTHORIZED(RequestDataAuthorized),
}

#[derive(PartialEq, Clone)]
pub struct RequestData {
    pub client_id: ClientId,
    pub client_auth: ClientAuth,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub expires_at: u32,
    pub device_id: Option<DeviceId>,
    pub sub: Option<Sub>,
    pub code: Option<Code>,
}

#[derive(PartialEq, Clone)]
pub struct RequestDataAuthorized {
    pub client_id: ClientId,
    pub client_auth: ClientAuth,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub expires_at: String,
    pub device_id: DeviceId,
    pub sub: Sub,
    pub code: Option<Code>,
}

pub fn is_request_data_authorized(data: RequestDataEnum) -> bool {
    match data {
        RequestDataEnum::UNAUTHORIZED(res) => false,
        RequestDataEnum::AUTHORIZED(res) => true,
    }
}
