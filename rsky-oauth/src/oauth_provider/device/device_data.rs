use crate::oauth_provider::device::session_id::SessionId;
use chrono::{DateTime, Utc};
use std::net::IpAddr;

pub struct DeviceData {
    pub user_agent: Option<String>,
    pub ip_address: IpAddr,
    pub session_id: SessionId,
    pub last_seen_at: DateTime<Utc>,
}
