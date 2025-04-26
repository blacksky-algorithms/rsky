use crate::oauth_provider::device::device_data::DeviceData;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::device::session_id::SessionId;
use crate::oauth_provider::errors::OAuthError;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;

pub struct PartialDeviceData {
    pub user_agent: Option<String>,
    pub ip_address: Option<IpAddr>,
    pub session_id: Option<SessionId>,
    pub last_seen_at: Option<i64>,
}

pub trait DeviceStore: Send + Sync {
    fn create_device(
        &mut self,
        device_id: DeviceId,
        data: DeviceData,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    fn read_device(
        &self,
        device_id: DeviceId,
    ) -> Pin<Box<dyn Future<Output = Result<Option<DeviceData>, OAuthError>> + Send + Sync + '_>>;
    fn update_device(
        &mut self,
        device_id: DeviceId,
        data: PartialDeviceData,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    fn delete_device(
        &mut self,
        device_id: DeviceId,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
}
