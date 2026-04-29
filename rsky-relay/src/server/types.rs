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

#[derive(Debug, Serialize, Deserialize)]
pub struct GetHostStatus {
    pub hostname: String,
    pub seq: u64,
    pub status: HostStatus,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BannedHost {
    pub host: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListBans {
    pub banned_hosts: Vec<BannedHost>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_serializes_camel_case() {
        let h = Host {
            account_count: 1,
            hostname: "pds.example".to_owned(),
            seq: 42,
            status: HostStatus::Active,
        };
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.contains("\"accountCount\":1"));
        assert!(json.contains("\"hostname\":\"pds.example\""));
        assert!(json.contains("\"seq\":42"));
        assert!(json.contains("\"status\":\"active\""));
    }

    #[test]
    fn host_status_round_trips_each_variant() {
        for (variant, expected) in [
            (HostStatus::Active, "active"),
            (HostStatus::Idle, "idle"),
            (HostStatus::Offline, "offline"),
            (HostStatus::Throttled, "throttled"),
            (HostStatus::Banned, "banned"),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let back: HostStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(format!("{back:?}"), format!("{variant:?}"));
        }
    }

    #[test]
    fn list_hosts_round_trips() {
        let lh = ListHosts {
            cursor: Some("c1".to_owned()),
            hosts: vec![Host {
                account_count: 0,
                hostname: "h".to_owned(),
                seq: 0,
                status: HostStatus::Idle,
            }],
        };
        let json = serde_json::to_string(&lh).unwrap();
        let back: ListHosts = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cursor.as_deref(), Some("c1"));
        assert_eq!(back.hosts.len(), 1);
    }

    #[test]
    fn get_host_status_round_trips() {
        let g = GetHostStatus { hostname: "x".to_owned(), seq: 7, status: HostStatus::Banned };
        let json = serde_json::to_string(&g).unwrap();
        let back: GetHostStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hostname, "x");
        assert_eq!(back.seq, 7);
    }

    #[test]
    fn banned_host_and_list_bans_round_trip() {
        let b = BannedHost {
            host: "evil.example".to_owned(),
            created_at: "2026-04-29T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&b).unwrap();
        let back: BannedHost = serde_json::from_str(&json).unwrap();
        assert_eq!(back.host, "evil.example");

        let lb = ListBans { banned_hosts: vec![back] };
        let json = serde_json::to_string(&lb).unwrap();
        let back: ListBans = serde_json::from_str(&json).unwrap();
        assert_eq!(back.banned_hosts.len(), 1);
    }
}
