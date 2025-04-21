use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::request::request_data::RequestData;
use crate::oauth_provider::request::request_id::RequestId;
use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthClientId};
use chrono::{DateTime, Utc};
use std::future::Future;
use std::pin::Pin;

#[derive(Default, Clone)]
pub struct UpdateRequestData {
    pub client_id: Option<OAuthClientId>,
    pub client_auth: Option<ClientAuth>,
    pub parameters: Option<OAuthAuthorizationRequestParameters>,
    pub expires_at: Option<DateTime<Utc>>,
    pub device_id: Option<DeviceId>,
    pub sub: Option<Sub>,
    pub code: Option<Code>,
}

pub struct FoundRequestResult {
    pub id: RequestId,
    pub data: RequestData,
}

pub trait RequestStore: Send + Sync {
    fn create_request(
        &mut self,
        id: RequestId,
        data: RequestData,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    /**
     * Note that expired requests **can** be returned to yield a different error
     * message than if the request was not found.
     */
    fn read_request(
        &self,
        id: &RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<Option<RequestData>, OAuthError>> + Send + Sync + '_>>;
    fn update_request(
        &mut self,
        id: RequestId,
        data: UpdateRequestData,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    fn delete_request(
        &mut self,
        id: RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    fn find_request_by_code(
        &self,
        code: Code,
    ) -> Pin<Box<dyn Future<Output = Option<FoundRequestResult>> + Send + Sync + '_>>;
}
