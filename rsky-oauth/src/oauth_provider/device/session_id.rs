use crate::oauth_provider::device::device_id::DeviceId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct SessionId(String);

//TODO generate hex id
pub async fn generate_session_id() -> SessionId {
    SessionId("test".to_string())
}
