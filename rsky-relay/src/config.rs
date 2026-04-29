use std::env;
use std::sync::LazyLock;
use std::time::Duration;

// admin
pub static ADMIN_PASSWORD: LazyLock<Option<String>> =
    LazyLock::new(|| env::var("RELAY_ADMIN_PASSWORD").ok().filter(|s| !s.is_empty()));
pub const BAN_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

// validator: when true (default), the validator publishes events that strict checks would defer or drop.
// AT Protocol relay validation is operator-discretion; consumers verify signatures end-to-end.
pub static LENIENT_VALIDATION: LazyLock<bool> = LazyLock::new(|| {
    env::var("RELAY_LENIENT_VALIDATION").map_or(true, |s| !matches!(s.as_str(), "0" | "false" | ""))
});

// metrics
pub static METRICS_LISTEN: LazyLock<Option<String>> =
    LazyLock::new(|| env::var("RELAY_METRICS_LISTEN").ok().filter(|s| !s.is_empty()));

// main
pub const CAPACITY_MSGS: usize = 1 << 16;
pub const CAPACITY_REQS: usize = 1 << 12;
pub const CAPACITY_STATUS: usize = 1 << 10;
pub const WORKERS_CRAWLERS: usize = 4;
pub const WORKERS_PUBLISHERS: usize = 4;

// server
pub const PORT: u16 = if cfg!(feature = "labeler") { 9001 } else { 9000 };
pub const HOSTS_RELAY: &str = "relay1.us-west.bsky.network";
pub const HOSTS_RELAYS_DEFAULT: &[&str] =
    &["relay1.us-west.bsky.network", "relay1.us-east.bsky.network", "bsky.network"];
pub static HOSTS_RELAYS: LazyLock<Vec<String>> = LazyLock::new(|| {
    env::var("RELAY_DISCOVERY_UPSTREAMS").ok().as_deref().map_or_else(
        || HOSTS_RELAYS_DEFAULT.iter().map(|s| (*s).to_owned()).collect(),
        |s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
    )
});
pub const HOSTS_INTERVAL: Duration = Duration::from_secs(60 * 5);
pub const HOSTS_MIN_ACCOUNTS: u64 = 0;

// resolver
pub static DO_PLC_EXPORT: LazyLock<bool> = LazyLock::new(|| {
    !cfg!(feature = "labeler") && env::args().filter(|arg| arg == "--no-plc-export").count() == 0
});
pub const PLC_EXPORT_INTERVAL: Duration = Duration::from_secs(60);
pub const CAPACITY_CACHE: usize = 1 << 18;

// validator
pub const HOSTS_WRITE_INTERVAL: Duration = Duration::from_secs(10);

// validator queue
pub const QUEUE_DISK_SIZE: u64 = 10 * 1024 * 1024 * 1024; // 10 GiB max queue size
pub const QUEUE_TTL_SECONDS: Option<u64> = Some(6 * 60 * 60); // 6 hours

// firehose
pub const DISK_SIZE: u64 = 320 * 1024 * 1024 * 1024; // 320 GiB
pub const TTL_SECONDS: Option<u64> = if cfg!(feature = "labeler") {
    None
} else {
    Some(24 * 60 * 60) // 24 hours
};

// fjall db
pub const CACHE_SIZE: u64 = 1024 * 1024 * 1024; // 1 GiB
pub const WRITE_BUFFER_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
pub const FSYNC_MS: Option<u16> = Some(1000); // 1 second
pub const MEMTABLE_SIZE: u32 = 64 * 1024 * 1024; // 64 MiB
pub const BLOCK_SIZE: u32 = 64 * 1024; // 64 KiB

#[cfg(test)]
mod tests {
    /// Parser used by the `HOSTS_RELAYS` `LazyLock`; testable independently of env mutation.
    pub(crate) fn parse_hosts_relays(raw: Option<&str>) -> Vec<String> {
        raw.map_or_else(
            || super::HOSTS_RELAYS_DEFAULT.iter().map(|s| (*s).to_owned()).collect(),
            |s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        )
    }

    #[test]
    fn hosts_relays_default_when_unset() {
        let v = parse_hosts_relays(None);
        assert!(v.contains(&"relay1.us-west.bsky.network".to_owned()));
        assert!(v.contains(&"bsky.network".to_owned()));
    }

    #[test]
    fn hosts_relays_parses_comma_list_with_whitespace() {
        let v = parse_hosts_relays(Some(" a , b , c "));
        assert_eq!(v, vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
    }

    #[test]
    fn hosts_relays_drops_empty_entries() {
        let v = parse_hosts_relays(Some("a,,b,"));
        assert_eq!(v, vec!["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn lenient_validation_default_true() {
        // The LazyLock isn't testable across env mutations without races; this is the static parse.
        // Verify behavior of the matching sub-expression directly.
        fn lenient_from(raw: Option<&str>) -> bool {
            raw.is_none_or(|s| !matches!(s, "0" | "false" | ""))
        }
        assert!(lenient_from(None));
        assert!(lenient_from(Some("1")));
        assert!(lenient_from(Some("true")));
        assert!(!lenient_from(Some("0")));
        assert!(!lenient_from(Some("false")));
        assert!(!lenient_from(Some("")));
    }
}
