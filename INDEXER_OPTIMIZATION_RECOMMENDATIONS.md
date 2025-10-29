# Indexer Performance Optimization Recommendations

## Current State
The indexer uses similar patterns to rsky-firehose (tokio async + Semaphore + connection pooling) but has room for optimization to handle "billions of records" at high intensity.

## Priority 1: Database Connection Pool Configuration

### Add to IndexerConfig
```rust
pub struct IndexerConfig {
    // ... existing fields
    pub db_pool_max_size: usize,
    pub db_pool_min_idle: usize,
}
```

### Update bin/indexer.rs
```rust
let pool_max_size = env::var("DB_POOL_MAX_SIZE")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(config.concurrency * 2);

let pool_min_idle = env::var("DB_POOL_MIN_IDLE")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(config.concurrency / 2);

let mut pg_config = Config::new();
pg_config.url = Some(config.database_url.clone());
pg_config.max_size = pool_max_size;
pg_config.min_idle = Some(pool_min_idle);
pg_config.manager = Some(ManagerConfig {
    recycling_method: RecyclingMethod::Fast,
});
pg_config.timeouts = Timeouts {
    wait: Some(Duration::from_secs(5)),
    create: Some(Duration::from_secs(5)),
    recycle: Some(Duration::from_secs(1)),
};
```

**Rationale**:
- PostgreSQL can handle many concurrent connections efficiently
- Rule of thumb: max_size = 2x concurrency allows for spikes
- min_idle keeps connections warm for consistent performance

## Priority 2: Increase Default Concurrency

### Current
```rust
concurrency: 10,  // Too low for billions of records
batch_size: 100,
```

### Recommended
```rust
concurrency: 100,   // Match rsky-firehose
batch_size: 500,    // Larger batches for throughput
```

**Rationale**:
- Modern systems can handle 100+ concurrent tasks easily
- Batching reduces Redis round-trips
- Network I/O benefits from parallelism

## Priority 3: Non-Blocking Batch Processing

### Current Pattern (Blocking)
```rust
// Process messages concurrently
let mut handles = Vec::new();
for message in messages {
    let handle = tokio::spawn(async move { ... });
    handles.push(handle);
}

// BLOCKS here waiting for ALL to complete
for handle in handles {
    let _ = handle.await;
}
```

### Recommended Pattern (Pipeline)
```rust
// Option A: Use FuturesUnordered for continuous processing
use futures::stream::{FuturesUnordered, StreamExt};

let mut tasks = FuturesUnordered::new();

loop {
    // Fetch new batch while old tasks complete
    tokio::select! {
        result = tasks.next() => {
            // Handle completed task
        }
        messages = read_batch() => {
            for msg in messages {
                let permit = semaphore.acquire_owned().await?;
                tasks.push(tokio::spawn(async move {
                    // Process
                    drop(permit);
                }));
            }
        }
    }
}

// Option B: Separate read and process loops
// Similar to rsky-firehose WebSocket pattern
```

**Rationale**:
- Keeps pipeline full at all times
- Reduces idle time between batches
- Better throughput for sustained high load

## Priority 4: Add Explicit Pool Configuration for IdResolver

### Current
```rust
let id_resolver = Arc::new(Mutex::new(rsky_identity::IdResolver::new(resolver_opts)));
```

### Recommended
Make IdResolver connections configurable:
```rust
// In rsky-identity, allow pool configuration
let resolver_opts = rsky_identity::types::IdentityResolverOpts {
    timeout: Some(Duration::from_secs(10)),
    plc_url: env::var("PLC_URL").ok(),
    did_cache: Some(DidCache::new(
        Some(Duration::from_secs(3600)),  // 1 hour stale
        Some(Duration::from_secs(86400)), // 24 hour max
    )),
    backup_nameservers: None,
};
```

**Rationale**:
- DID resolution can be slow (external HTTP calls)
- Caching reduces external dependencies
- Timeouts prevent blocking on slow responses

## Priority 5: Add Metrics and Monitoring

### Add Real Metrics Collection
```rust
use std::sync::atomic::{AtomicU64, Ordering};

pub struct IndexerMetrics {
    pub processed: AtomicU64,
    pub failed: AtomicU64,
    pub in_flight: AtomicU64,
    pub batch_duration_ms: AtomicU64,
    pub db_query_duration_ms: AtomicU64,
}

impl IndexerMetrics {
    pub fn record_processed(&self) {
        self.processed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            processed: self.processed.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            in_flight: self.in_flight.load(Ordering::Relaxed),
            // ...
        }
    }
}
```

**Rationale**:
- Essential for production monitoring
- Helps identify bottlenecks
- Enables adaptive concurrency

## Priority 6: Consider Multi-Instance Scaling

### Current: Single process with configurable concurrency
### Recommended: Support horizontal scaling

```bash
# Run multiple indexer instances with different consumer names
CONSUMER_NAME=indexer_1 INDEXER_CONCURRENCY=100 ./indexer &
CONSUMER_NAME=indexer_2 INDEXER_CONCURRENCY=100 ./indexer &
CONSUMER_NAME=indexer_3 INDEXER_CONCURRENCY=100 ./indexer &
```

Redis consumer groups automatically distribute work across consumers.

**Rationale**:
- Redis consumer groups support multiple consumers
- Scales horizontally across machines
- Better fault tolerance

## Comparison Summary

| Feature | rsky-relay | rsky-firehose | rsky-indexer (current) | rsky-indexer (recommended) |
|---------|-----------|---------------|------------------------|---------------------------|
| **Concurrency Model** | OS threads | Tokio async | Tokio async | Tokio async |
| **Max Concurrent** | N workers | 100 (Semaphore) | 10 (Semaphore) | 100+ (configurable) |
| **Connection Pooling** | Per-thread SQLite | HTTP pool (10/host) | PG pool (default) + Redis mgr | Explicit PG pool (2x concurrency) |
| **Batch Processing** | Lock-free channels | Continuous pipeline | Wait-for-batch | Continuous pipeline |
| **Metrics** | Basic | None | Placeholder | Real atomic counters |

## Implementation Priority

1. ⚡ **Immediate**: Add DB pool size configuration (5 min)
2. ⚡ **High**: Increase default concurrency to 100 (1 min)
3. 🔧 **Medium**: Implement non-blocking pipeline (30 min)
4. 📊 **Medium**: Add real metrics (1 hour)
5. 🚀 **Low**: Document multi-instance setup (30 min)

## Expected Performance Impact

- **DB Pool Sizing**: +30% throughput (reduces connection wait time)
- **Higher Concurrency**: +10x throughput (10 → 100 concurrent)
- **Non-blocking Pipeline**: +20% throughput (eliminates batch gaps)
- **Combined**: ~13x improvement in sustained throughput

For "billions of records", these optimizations are **critical**.
