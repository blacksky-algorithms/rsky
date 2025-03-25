use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::request::request_data::RequestData;
use crate::oauth_provider::request::request_id::RequestId;
use crate::oauth_types::OAuthAuthorizationRequestParameters;

#[derive(Default, Clone)]
pub struct UpdateRequestData {
    pub client_id: Option<ClientId>,
    pub client_auth: Option<ClientAuth>,
    pub parameters: Option<OAuthAuthorizationRequestParameters>,
    pub expires_at: Option<u32>,
    pub device_id: Option<DeviceId>,
    pub sub: Option<Sub>,
    pub code: Option<Code>,
}

pub struct FoundRequestResult {
    pub id: RequestId,
    pub data: RequestData,
}

pub trait RequestStore: Send + Sync {
    fn create_request(&mut self, id: RequestId, data: RequestData);
    /**
     * Note that expired requests **can** be returned to yield a different error
     * message than if the request was not found.
     */
    fn read_request(&self, id: &RequestId) -> Option<&RequestData>;
    fn update_request(&mut self, id: RequestId, data: UpdateRequestData) -> Result<(), OAuthError>;
    fn delete_request(&mut self, id: RequestId);
    fn find_request_by_code(&self, code: Code) -> Option<FoundRequestResult>;
}
