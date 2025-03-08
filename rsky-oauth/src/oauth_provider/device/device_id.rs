use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct DeviceId(String);

//TODO generate hex id
pub async fn generate_device_id() -> DeviceId {
    DeviceId("test".to_string())
}
