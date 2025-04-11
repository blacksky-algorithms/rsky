use crate::oauth_provider::device::session_id::SessionId;

pub struct DeviceData {
    pub user_agent: Option<String>,
    pub ip_address: String,
    pub session_id: SessionId,
    pub last_seen_at: u64,
}
