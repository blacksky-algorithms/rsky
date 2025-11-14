# Production Deployment Plan - Critical Fixes

**Date**: November 3-4, 2025
**Branch**: rude1/backfill
**Commits**: c50cdc2, ff5c5d2, 107b113, 5355e74

## Critical Fixes to Deploy

### 1. Fix Firehose Ingester Stream Pollution (HIGHEST PRIORITY)
**Commit**: c50cdc2
**File**: rsky-ingester/src/firehose.rs
**Problem**: firehose_live stream was polluted with `type: repo` events, causing:
- 4,658 events/sec instead of expected ~1,000/sec (4.6x too high)
- Stream at constant high water mark (40M events)
- Only 2.5 hours of data retained despite 40M capacity
- New posts being trimmed before indexing could process them
- Missing posts like `at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4ra7cs75s2z`

**Root Cause**: Lines 352-359 were erroneously adding repo events after processing commits. This was a misinterpretation of AT Protocol Sync v1.1 - the ingester should only emit filtered record operations (create/update/delete), not repo metadata events.

**Fix**: Removed the erroneous repo event push from firehose.rs

**Impact**:
- Reduces event rate to expected ~1,000/sec
- Eliminates constant backpressure
- Allows proper retention of posts for indexing
- Fixes missing posts issue

**Binary**: `target/release/ingester`

---

### 2. Fix Record Table Revision Checking
**Commit**: ff5c5d2
**File**: rsky-indexer/src/indexing/mod.rs:673
**Problem**: Record table INSERT...ON CONFLICT was missing WHERE clause for revision checking, allowing stale backfill data to overwrite newer live data

**Root Cause**: Race condition between live firehose indexing and backfill processing. When backfill processed old revisions after newer ones were already indexed, it would overwrite with stale data.

**Fix**: Added `WHERE record.rev <= EXCLUDED.rev` to the ON CONFLICT clause

**Impact**:
- Prevents stale writes from overwriting newer data
- Ensures revision ordering is respected
- Critical for data integrity during backfill operations

**Binary**: `target/release/indexer`

---

### 3. Fix Backfill Ingester Infinite Retry Loop
**Commit**: 107b113
**File**: rsky-ingester/src/backfill.rs:226
**Problem**: When listRepos API failed 3 times on a specific cursor, the ingester would skip by +1, landing on the same broken batch and retrying forever

**Root Cause**: Cursor skip logic incremented by 1 instead of the batch size (1000), so it would skip to another cursor in the same problematic batch

**Fix**: Changed cursor skip from +1 to +1000 to skip the entire batch

**Impact**:
- Prevents infinite retry loops on broken batches
- Allows backfill to progress past relay-side issues
- Improves resilience to data quality issues

**Binary**: `target/release/ingester`

---

### 4. Fix Metrics Initialization
**Commit**: 5355e74
**Files**:
- rsky-backfiller/src/metrics.rs
- rsky-backfiller/src/bin/backfiller.rs
- rsky-ingester/src/metrics.rs
- rsky-ingester/src/bin/ingester.rs

**Problem**: 9 metrics missing from /metrics endpoints despite being defined in code, causing Grafana "No data" panels

**Root Cause**: Rust's lazy_static! only registers metrics on first access. Error metrics that hadn't been triggered weren't registered with Prometheus.

**Fix**: Added initialize_metrics() functions that force-access all metric references at startup

**Impact**: All Grafana dashboard panels now show data correctly

**Binaries**: `target/release/ingester`, `target/release/backfiller`

---

## Deployment Order

### Phase 1: Stop Services (in order)
```bash
# On production server via SSH
sudo systemctl stop rsky-indexer    # Stop consumer first
sudo systemctl stop rsky-ingester   # Then stop producer
sudo systemctl stop rsky-backfiller # Optional - no urgent changes
```

### Phase 2: Backup Current Binaries
```bash
sudo cp /usr/local/bin/ingester /usr/local/bin/ingester.backup.$(date +%Y%m%d_%H%M%S)
sudo cp /usr/local/bin/indexer /usr/local/bin/indexer.backup.$(date +%Y%m%d_%H%M%S)
sudo cp /usr/local/bin/backfiller /usr/local/bin/backfiller.backup.$(date +%Y%m%d_%H%M%S)
```

### Phase 3: Copy New Binaries
```bash
# From local machine
scp target/release/ingester production:/tmp/ingester.new
scp target/release/indexer production:/tmp/indexer.new
scp target/release/backfiller production:/tmp/backfiller.new

# On production server
sudo mv /tmp/ingester.new /usr/local/bin/ingester
sudo mv /tmp/indexer.new /usr/local/bin/indexer
sudo mv /tmp/backfiller.new /usr/local/bin/backfiller
sudo chmod +x /usr/local/bin/{ingester,indexer,backfiller}
```

### Phase 4: Clean Polluted Stream
**CRITICAL**: Must clean firehose_live stream to remove polluted data

```bash
# Connect to Redis
redis-cli -h localhost -p 6380

# Check current length
XLEN firehose_live
# Expected: ~40,000,000

# Option A: Trim to keep only last 1 hour (recommended for minimal downtime)
# At 1000/sec, 1 hour = 3,600,000 events
XTRIM firehose_live MAXLEN ~ 3600000

# Option B: Delete and recreate (cleanest, but higher downtime risk)
DEL firehose_live
# Stream will be auto-created on first XADD
```

### Phase 5: Start Services (in order)
```bash
# Start producer first
sudo systemctl start rsky-ingester
sudo systemctl status rsky-ingester
sudo journalctl -u rsky-ingester -f &

# Verify metrics initialization log appears:
# Expected: "Metrics initialized"

# Then start consumer
sudo systemctl start rsky-indexer
sudo systemctl status rsky-indexer

# Start backfiller (if stopped)
sudo systemctl start rsky-backfiller
```

### Phase 6: Verify Deployment
```bash
# Check metrics endpoints
curl http://localhost:9090/metrics | grep ingester_firehose_messages_total
curl http://localhost:9091/metrics | grep indexer_records_inserted_total
curl http://localhost:9092/metrics | grep backfiller_repos_processed_total

# Monitor firehose_live stream length
redis-cli -h localhost -p 6380 XLEN firehose_live
# Should stay below high water mark (100,000 for ingester)

# Check event rate (repeat after 60 seconds)
# First reading
redis-cli -h localhost -p 6380 XINFO STREAM firehose_live | grep "last-generated-id"
# Wait 60 seconds
redis-cli -h localhost -p 6380 XINFO STREAM firehose_live | grep "last-generated-id"
# Calculate difference in seq numbers / 60 = events/sec
# Expected: ~1,000/sec (NOT 4,658/sec)

# Verify no repo events in stream
redis-cli -h localhost -p 6380 XRANGE firehose_live - + COUNT 100 | grep "type.*repo"
# Expected: No matches (only create/update/delete events)

# Check Grafana dashboard
# All panels should show data
# firehose_live stream length should be stable, not maxed out
```

### Phase 7: Monitor for Issues
Monitor for 30 minutes after deployment:
- Stream length stays below high water mark
- Event rate ~1,000/sec (not 4,658/sec)
- No infinite retry loops in ingester logs
- New posts appear in database
- Grafana metrics update correctly

---

## Rollback Plan

If issues occur:
```bash
# Stop services
sudo systemctl stop rsky-indexer
sudo systemctl stop rsky-ingester
sudo systemctl stop rsky-backfiller

# Restore backup binaries
sudo cp /usr/local/bin/ingester.backup.TIMESTAMP /usr/local/bin/ingester
sudo cp /usr/local/bin/indexer.backup.TIMESTAMP /usr/local/bin/indexer
sudo cp /usr/local/bin/backfiller.backup.TIMESTAMP /usr/local/bin/backfiller

# Restart services
sudo systemctl start rsky-ingester
sudo systemctl start rsky-indexer
sudo systemctl start rsky-backfiller
```

---

## Expected Improvements Post-Deployment

1. **Event Rate**: Drops from 4,658/sec to ~1,000/sec
2. **Stream Retention**: Can retain ~11 hours of data instead of 2.5 hours (at 1,000/sec with 40M capacity)
3. **Missing Posts**: Fixed - new posts will no longer be trimmed before indexing
4. **Backpressure**: Eliminated - stream won't constantly hit high water mark
5. **Data Integrity**: Backfill can't overwrite newer live data
6. **Resilience**: Backfill can skip past broken batches instead of infinite retry
7. **Observability**: All 9 missing metrics now visible in Grafana

---

## Post-Deployment Tasks

1. Monitor metrics for 24 hours
2. Verify missing posts like `at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4ra7cs75s2z` are now indexed
3. Check backfill progress resumes normally
4. Update runbook with lessons learned
5. Consider adjusting high water mark if needed based on new event rate
