# Handle Resolution Efficiency Plan

## Current State
- **8.5M actors** with NULL handles
- **461K actors** with valid handles
- Concurrency: 10 workers (default)
- Batch size: 500 (default)
- Estimated rate: ~5-10 handles/second
- Time to clear backlog at current rate: **10-20 days**

## Current Bottlenecks

### 1. HTTP Latency Per Resolution
Each handle resolution requires 2 HTTP requests:
- PLC directory lookup: `https://plc.directory/{did}` (~50-200ms)
- DNS/HTTP verification: `https://{handle}/.well-known/atproto-did` (~50-500ms)

### 2. Low Concurrency
Default 10 concurrent workers is conservative. Network-bound tasks can scale higher.

### 3. No Negative Caching
Failed resolutions (deleted accounts, invalid handles) are retried after 1 hour.
With 8.5M actors, many are deleted/invalid and waste cycles.

### 4. No PDS-Specific Prioritization
blacksky.app users are mixed in with millions of other PDS users.

### 5. Single-Record DB Updates
Each resolved handle does individual INSERT/UPDATE.

---

## Proposed Improvements

### Phase 1: Quick Wins (No Code Changes)

#### 1.1 Increase Concurrency
```bash
# In wintermute service environment
HANDLE_RESOLUTION_CONCURRENCY=100
HANDLE_RESOLUTION_BATCH_SIZE=1000
```
Expected improvement: **10x throughput**

#### 1.2 Increase Invalid Reindex Interval
Reduce wasted cycles on failed DIDs by extending retry interval.
```rust
// config.rs - change from 1 hour to 7 days
pub const HANDLE_REINDEX_INTERVAL_INVALID: Duration = Duration::from_secs(7 * 24 * 60 * 60);
```

### Phase 2: PLC Export Bulk Resolution

#### 2.1 Use PLC Directory Export
PLC provides a full export at `https://plc.directory/export`
- Download entire directory (~2GB compressed)
- Parse locally and bulk-update actors table
- Skip HTTP lookups entirely for initial resolution

```rust
// New module: src/handle_bulk_import.rs
async fn import_plc_export(pool: &Pool) -> Result<usize, WintermuteError> {
    // Stream PLC export
    // Parse each line as DID document
    // Extract handle from alsoKnownAs
    // Bulk INSERT/UPDATE to actor table
}
```

**Expected time**: 1-2 hours for initial 8.5M actors

#### 2.2 Verification Queue
After bulk import, queue handles for async verification:
- Only verify handles that appear in user-facing queries (lazy verification)
- Or background-verify in priority order

### Phase 3: Smart Prioritization

#### 3.1 PDS-Specific Priority Queue
Add column to track PDS origin:
```sql
ALTER TABLE actor ADD COLUMN pds_host VARCHAR;
CREATE INDEX actor_pds_host_idx ON actor(pds_host) WHERE handle IS NULL;
```

Allow priority resolution by PDS:
```rust
// New function
async fn resolve_handles_for_pds(&self, pds_host: &str) -> Result<usize, WintermuteError>
```

#### 3.2 On-Demand Resolution
Resolve handles when actors appear in queries, not proactively:
- In getProfile: resolve if NULL
- In getAuthorFeed: resolve actors as they appear
- Cache resolved handles for 24h

### Phase 4: Negative Caching

#### 4.1 Track Failed Resolutions
```sql
ALTER TABLE actor ADD COLUMN handle_resolution_failed_at TIMESTAMP;
ALTER TABLE actor ADD COLUMN handle_resolution_failure_count INT DEFAULT 0;
```

Skip DIDs that have failed 3+ times until manual intervention.

#### 4.2 Tombstone Detection
Check PLC for tombstoned DIDs and mark as permanently unresolvable:
```rust
if did_doc.is_tombstoned() {
    // Mark actor as deleted, skip future resolution
}
```

### Phase 5: Batch Database Operations

#### 5.1 Bulk Updates
Collect resolved handles in memory, write in batches:
```rust
// Instead of individual UPDATEs
let batch: Vec<(String, String)> = resolved_handles;
client.execute(
    "UPDATE actor AS a SET handle = v.handle, \"indexedAt\" = NOW()
     FROM (VALUES ($1)) AS v(did, handle) WHERE a.did = v.did",
    &[&batch]
).await?;
```

### Phase 6: HTTP Client Optimization

#### 6.1 Connection Pooling
Use reqwest connection pool for PLC and handle verification:
```rust
let http_client = reqwest::Client::builder()
    .pool_max_idle_per_host(50)
    .pool_idle_timeout(Duration::from_secs(30))
    .timeout(Duration::from_secs(5))
    .build()?;
```

#### 6.2 DNS Caching
Enable system DNS cache or use trust-dns-resolver with caching.

---

## Implementation Priority

| Phase | Effort | Impact | Priority |
|-------|--------|--------|----------|
| 1.1 Increase concurrency | 5 min | 10x | **NOW** |
| 1.2 Extend invalid interval | 5 min | 2x | **NOW** |
| 2.1 PLC bulk import | 4 hours | 100x initial | **HIGH** |
| 3.1 PDS priority | 2 hours | UX | **HIGH** |
| 3.2 On-demand resolution | 4 hours | UX | MEDIUM |
| 4.1 Negative caching | 2 hours | 2x | MEDIUM |
| 5.1 Batch updates | 2 hours | 1.5x | LOW |
| 6.1 HTTP optimization | 1 hour | 1.2x | LOW |

---

## Immediate Actions

1. **Bump concurrency** on production wintermute:
   ```bash
   HANDLE_RESOLUTION_CONCURRENCY=100
   HANDLE_RESOLUTION_BATCH_SIZE=1000
   ```

2. **Create PLC import script** to bulk-load handles from export

3. **Add blacksky.app priority** - query actors by PDS and resolve first

---

## Metrics to Add

```rust
// Handle resolution metrics
static HANDLES_RESOLVED_TOTAL: Counter = ...;
static HANDLES_FAILED_TOTAL: Counter = ...;
static HANDLE_RESOLUTION_DURATION: Histogram = ...;
static HANDLES_PENDING_GAUGE: Gauge = ...;
```
