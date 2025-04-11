use crate::oauth_provider::device::device_data::DeviceData;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use std::future::Future;
use std::pin::Pin;

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
        data: DeviceData,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    fn delete_device(
        &mut self,
        device_id: DeviceId,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
}
