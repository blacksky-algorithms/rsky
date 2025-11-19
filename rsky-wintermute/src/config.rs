use std::time::Duration;

pub const CAPACITY_FIREHOSE: usize = 1 << 16;
pub const CAPACITY_BACKFILL: usize = 1 << 14;
pub const CAPACITY_INDEX: usize = 1 << 14;

pub const WORKERS_INGESTER: usize = 4;
pub const WORKERS_BACKFILLER: usize = 8;
pub const WORKERS_INDEXER: usize = 8;

pub const CACHE_SIZE: u64 = 2 * 1024 * 1024 * 1024;
pub const WRITE_BUFFER_SIZE: u64 = 512 * 1024 * 1024;
pub const FSYNC_MS: Option<u16> = Some(1000);
pub const MEMTABLE_SIZE: u32 = 64 * 1024 * 1024;
pub const BLOCK_SIZE: u32 = 64 * 1024;

pub const FIREHOSE_PING_INTERVAL: Duration = Duration::from_secs(30);
pub const BACKFILLER_TIMEOUT: Duration = Duration::from_secs(60);
pub const INDEXER_BATCH_SIZE: usize = 500;
pub const BACKFILLER_BATCH_SIZE: usize = 100;
