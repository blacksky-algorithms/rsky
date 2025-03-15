use crate::oauth_provider::device::device_data::DeviceData;
use crate::oauth_provider::device::device_id::DeviceId;

pub struct DeviceStore {}

impl DeviceStore {
    pub async fn create_device(&mut self, device_id: DeviceId, data: DeviceData) {
        unimplemented!()
    }
    pub async fn read_device(&self, device_id: DeviceId) -> Option<DeviceData> {
        unimplemented!()
    }
    pub async fn update_device(&mut self, device_id: DeviceId, data: DeviceData) {
        unimplemented!()
    }
    pub async fn delete_device(&mut self, device_id: DeviceId) {
        unimplemented!()
    }
}
