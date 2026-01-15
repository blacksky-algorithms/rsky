use std::sync::LazyLock;
use std::time::Duration;

pub const CAPACITY_FIREHOSE: usize = 1 << 16;
pub const CAPACITY_BACKFILL: usize = 1 << 14;
pub const CAPACITY_INDEX: usize = 1 << 14;

pub const WORKERS_INGESTER: usize = 4;

// Fjall storage config - tunable via environment variables
// On high-memory servers (200GB+ RAM), these should be increased significantly
// Rule of thumb: CACHE_SIZE = 20-25% of RAM, WRITE_BUFFER_SIZE = 1-2% of RAM
pub static CACHE_SIZE: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("FJALL_CACHE_SIZE_GB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map_or(32 * 1024 * 1024 * 1024, |gb| gb * 1024 * 1024 * 1024) // Default: 32GB
});

pub static WRITE_BUFFER_SIZE: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("FJALL_WRITE_BUFFER_SIZE_GB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map_or(2 * 1024 * 1024 * 1024, |gb| gb * 1024 * 1024 * 1024) // Default: 2GB
});

pub const FSYNC_MS: Option<u16> = Some(1000);
pub const MEMTABLE_SIZE: u32 = 256 * 1024 * 1024; // 256MB (up from 64MB)
pub const BLOCK_SIZE: u32 = 64 * 1024;

pub const FIREHOSE_PING_INTERVAL: Duration = Duration::from_secs(30);

// Cursor save interval - like indigo/tap's cursorSaveInterval
// Saves cursor to Fjall/Postgres periodically instead of every event
// This prevents Fjall poisoning from high-frequency writes
pub static CURSOR_SAVE_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    let secs = std::env::var("CURSOR_SAVE_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5); // Default: save cursor every 5 seconds
    Duration::from_secs(secs)
});

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

// Number of parallel batch processors for backfill indexing
// Each worker dequeues and processes batches independently
// Should be tuned based on DB pool size (e.g., pool_size / 2)
pub static INDEXER_BATCH_WORKERS: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INDEXER_BATCH_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4) // Default: 4 parallel batch workers
});

// Maximum concurrent indexer tasks for backfill processing
// Higher values can increase throughput but also increase DB connection contention
// Should be tuned based on DB pool size and available resources
pub static INDEXER_MAX_CONCURRENT: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INDEXER_MAX_CONCURRENT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200) // Default: 200 concurrent tasks (increased from 50)
});

// Handle resolution: revalidate handles after this duration
pub const HANDLE_REINDEX_INTERVAL_VALID: Duration = Duration::from_secs(24 * 60 * 60); // 1 day
pub const HANDLE_REINDEX_INTERVAL_INVALID: Duration = Duration::from_secs(60 * 60); // 1 hour
pub const IDENTITY_RESOLVER_TIMEOUT: Duration = Duration::from_secs(3);

// Handle resolution concurrency - process multiple handles in parallel
pub static HANDLE_RESOLUTION_CONCURRENCY: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("HANDLE_RESOLUTION_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50) // Default: 50 concurrent handle resolutions
});

pub static HANDLE_RESOLUTION_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("HANDLE_RESOLUTION_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500) // Default: 500 actors per batch
});

// Priority window for recently-indexed actors (resolve new actors faster)
pub const HANDLE_PRIORITY_WINDOW: Duration = Duration::from_secs(6 * 60 * 60); // 6 hours

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

// Inline processing concurrency for firehose events
// Should be proportional to DB_POOL_SIZE to avoid excessive connection contention
pub static INLINE_CONCURRENCY: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INLINE_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100) // Default: 100 concurrent inline indexing tasks (5x pool size)
});

// Database pool size per component (firehose, labels, indexer, backfiller each get a pool)
// With 4 pools, default 20 each = 80 connections, leaving headroom under Postgres default 100
pub static DB_POOL_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("DB_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20) // Default: 20 connections per pool (80 total across 4 pools)
});

// Backfiller direct write mode - bypass Fjall queue and write directly to PostgreSQL
// This eliminates the Fjall dequeue bottleneck (~3.5s per batch) for backfill operations
pub static BACKFILLER_DIRECT_WRITE: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("BACKFILLER_DIRECT_WRITE")
        .ok()
        .is_none_or(|s| s == "true" || s == "1") // Default: enabled (bypass Fjall)
});

// Backfiller DB pool size - separate from main pool for direct write mode
pub static BACKFILLER_DB_POOL_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BACKFILLER_DB_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(32) // Default: 32 connections for backfiller (matches worker count)
});
