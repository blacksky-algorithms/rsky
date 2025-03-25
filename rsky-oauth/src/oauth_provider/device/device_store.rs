use crate::oauth_provider::device::device_data::DeviceData;
use crate::oauth_provider::device::device_id::DeviceId;

pub trait DeviceStore: Send + Sync {
    fn create_device(&mut self, device_id: DeviceId, data: DeviceData);
    fn read_device(&self, device_id: DeviceId) -> Option<DeviceData>;
    fn update_device(&mut self, device_id: DeviceId, data: DeviceData);
    fn delete_device(&mut self, device_id: DeviceId);
}
