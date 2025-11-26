use std::time::Duration;

pub const CAPACITY_FIREHOSE: usize = 1 << 16;
pub const CAPACITY_BACKFILL: usize = 1 << 14;
pub const CAPACITY_INDEX: usize = 1 << 14;

pub const WORKERS_INGESTER: usize = 4;
pub const WORKERS_BACKFILLER: usize = 8;
pub const WORKERS_INDEXER: usize = 16;

pub const CACHE_SIZE: u64 = 2 * 1024 * 1024 * 1024;
pub const WRITE_BUFFER_SIZE: u64 = 512 * 1024 * 1024;
pub const FSYNC_MS: Option<u16> = Some(1000);
pub const MEMTABLE_SIZE: u32 = 64 * 1024 * 1024;
pub const BLOCK_SIZE: u32 = 64 * 1024;

pub const FIREHOSE_PING_INTERVAL: Duration = Duration::from_secs(30);
pub const BACKFILLER_TIMEOUT: Duration = Duration::from_secs(60);
pub const INDEXER_BATCH_SIZE: usize = 1000;
pub const BACKFILLER_BATCH_SIZE: usize = 100;

// Cursor staleness: if no message received within this time, reset cursor
pub const CURSOR_STALE_TIMEOUT: Duration = Duration::from_secs(30);

// Backpressure: pause backfiller when output stream exceeds this length
pub const BACKFILLER_OUTPUT_HIGH_WATER_MARK: usize = 10_000;

// Handle resolution: revalidate handles after this duration
pub const HANDLE_REINDEX_INTERVAL_VALID: Duration = Duration::from_secs(24 * 60 * 60); // 1 day
pub const HANDLE_REINDEX_INTERVAL_INVALID: Duration = Duration::from_secs(60 * 60); // 1 hour
pub const IDENTITY_RESOLVER_TIMEOUT: Duration = Duration::from_secs(3);
