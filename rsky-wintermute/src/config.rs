use std::sync::LazyLock;
use std::time::Duration;

pub const CAPACITY_FIREHOSE: usize = 1 << 16;
pub const CAPACITY_BACKFILL: usize = 1 << 14;
pub const CAPACITY_INDEX: usize = 1 << 14;

pub const WORKERS_INGESTER: usize = 4;

pub const CACHE_SIZE: u64 = 2 * 1024 * 1024 * 1024;
pub const WRITE_BUFFER_SIZE: u64 = 512 * 1024 * 1024;
pub const FSYNC_MS: Option<u16> = Some(1000);
pub const MEMTABLE_SIZE: u32 = 64 * 1024 * 1024;
pub const BLOCK_SIZE: u32 = 64 * 1024;

pub const FIREHOSE_PING_INTERVAL: Duration = Duration::from_secs(30);

// Indexer config - tunable via environment variables
pub static WORKERS_INDEXER: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INDEXER_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16) // Default: 16 concurrent index workers
});

pub static INDEXER_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INDEXER_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000) // Default: 1000 records per batch
});

// Handle resolution: revalidate handles after this duration
pub const HANDLE_REINDEX_INTERVAL_VALID: Duration = Duration::from_secs(24 * 60 * 60); // 1 day
pub const HANDLE_REINDEX_INTERVAL_INVALID: Duration = Duration::from_secs(60 * 60); // 1 hour
pub const IDENTITY_RESOLVER_TIMEOUT: Duration = Duration::from_secs(3);

// Backfiller config - tunable via environment variables for 15B+ record backfills
pub static WORKERS_BACKFILLER: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BACKFILLER_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(32) // Default: 32 concurrent repo fetches
});

pub static BACKFILLER_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BACKFILLER_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000) // Default: dequeue 1000 repos per batch
});

pub static BACKFILLER_OUTPUT_HIGH_WATER_MARK: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BACKFILLER_OUTPUT_HIGH_WATER_MARK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000) // Default: 100k records in output queue before backpressure
});

pub static BACKFILLER_TIMEOUT_SECS: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("BACKFILLER_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120) // Default: 2 minutes per repo fetch
});

#[must_use]
pub fn backfiller_timeout() -> Duration {
    Duration::from_secs(*BACKFILLER_TIMEOUT_SECS)
}
