use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ListHosts {
    pub cursor: Option<String>,
    pub hosts: Vec<Host>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub account_count: u64,
    pub hostname: String,
    pub seq: u64,
    pub status: HostStatus,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    Active,
    Idle,
    Offline,
    Throttled,
    Banned,
}
