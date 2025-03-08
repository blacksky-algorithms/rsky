use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::request::request_data::RequestData;
use crate::oauth_provider::request::request_id::RequestId;
use crate::oauth_types::OAuthAuthorizationRequestParameters;

pub struct UpdateRequestData {
    pub client_id: Option<ClientId>,
    pub client_auth: Option<ClientAuth>,
    pub parameters: Option<OAuthAuthorizationRequestParameters>,
    pub expires_at: Option<u32>,
    pub device_id: Option<Option<DeviceId>>,
    pub sub: Option<Option<Sub>>,
    pub code: Option<Option<Code>>,
}

pub struct FoundRequestResult {
    pub id: RequestId,
    pub data: RequestData,
}
