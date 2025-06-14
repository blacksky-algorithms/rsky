use std::env;
use std::sync::LazyLock;
use std::time::Duration;

// main
pub const CAPACITY_MSGS: usize = 1 << 16;
pub const CAPACITY_REQS: usize = 1 << 12;
pub const CAPACITY_STATUS: usize = 1 << 10;
pub const WORKERS_CRAWLERS: usize = 4;
pub const WORKERS_PUBLISHERS: usize = 4;

// server
pub const PORT: u16 = if cfg!(feature = "labeler") { 9001 } else { 9000 };
pub const HOSTS_RELAY: &str = "relay1.us-west.bsky.network";
pub const HOSTS_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub const HOSTS_MIN_ACCOUNTS: u64 = 0;

// resolver
pub static DO_PLC_EXPORT: LazyLock<bool> = LazyLock::new(|| {
    !cfg!(feature = "labeler") && env::args().filter(|arg| arg == "--no-plc-export").count() == 0
});
pub const PLC_EXPORT_INTERVAL: Duration = Duration::from_secs(60);
pub const CAPACITY_CACHE: usize = 1 << 18;

// validator
pub const HOSTS_WRITE_INTERVAL: Duration = Duration::from_secs(10);

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
