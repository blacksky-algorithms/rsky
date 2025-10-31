# Backfill System End-to-End Plan

## Goal
Get the complete backfill code running end to end:
1. **BackfillIngester** → listRepos on relay → write to `repo_backfill` stream
2. **RepoBackfiller** → consume `repo_backfill` → getRepos for DID at PDS URL → push to `firehose_backfill` stream
3. **StreamIndexer** → consume `firehose_backfill` → write to PostgreSQL

## Current Status

### ✅ Working Components
- **FirehoseIngester**: Successfully consuming live firehose from relays, writing to `firehose_live`
- **LabelerIngester**: Successfully consuming labels, writing to `label_live`
- **StreamIndexer**: Successfully consuming `firehose_live`, `firehose_backfill`, and `label_live`, writing to PostgreSQL

### ❌ Broken Components
- **BackfillIngester**: Stuck in retry loop when relay returns 500 errors for specific cursors
  - **Root Cause**: No error recovery mechanism, retries same cursor infinitely
  - **Fix Applied**: Added retry counter with skip-ahead logic (/Users/rudyfraser/Projects/rsky/rsky-ingester/src/backfill.rs:129-208)

- **RepoBackfiller**: Status unknown, needs investigation

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    AT Protocol Relay                             │
│          (Firehose WebSocket + listRepos endpoint)               │
└────────────┬────────────────────────────────┬───────────────────┘
             │                                 │
             │ Firehose events                 │ Repo list (cursor-paginated)
             ▼                                 ▼
    ┌────────────────┐              ┌───────────────────┐
    │ FirehoseIngester│              │ BackfillIngester │
    │  (WORKING ✅)  │              │ (FIXED - TESTING)│
    └────────┬───────┘              └─────────┬─────────┘
             │                                 │
             │ writes events                   │ writes DIDs + PDS URLs
             ▼                                 ▼
    ┌─────────────────┐              ┌──────────────────┐
    │ firehose_live   │              │ repo_backfill    │
    │ (Redis Stream)  │              │ (Redis Stream)   │
    │  INCREASING ✅  │              │  STUCK AT 850K ❌│
    └─────────────────┘              └────────┬─────────┘
                                               │
                                               │ consumed by XREADGROUP
                                               ▼
                                     ┌───────────────────┐
                                     │ RepoBackfiller    │
                                     │  (UNKNOWN ❓)    │
                                     └─────────┬─────────┘
                                               │
                                               │ writes events from repo
                                               ▼
                                     ┌────────────────────┐
                                     │ firehose_backfill  │
                                     │ (Redis Stream)     │
                                     │  DRAINING ✅       │
                                     └────────────────────┘

    ┌─────────────────┐              ┌────────────────────┐
    │ firehose_live   │              │ firehose_backfill  │
    └────────┬────────┘              └─────────┬──────────┘
             │                                  │
             │ consumed by XREADGROUP           │ consumed by XREADGROUP
             └──────────┬───────────────────────┘
                        ▼
              ┌──────────────────────┐
              │  StreamIndexer(s)     │
              │  (Consumer Group)     │
              │    WORKING ✅         │
              └──────────┬────────────┘
                         │
                         │ writes to PostgreSQL
                         ▼
              ┌────────────────────────┐
              │   PostgreSQL           │
              │   (bsky database)      │
              │  RECEIVING DATA ✅     │
              └────────────────────────┘
```

## Component Details

### 1. BackfillIngester

**File**: `/Users/rudyfraser/Projects/rsky/rsky-ingester/src/backfill.rs`

**Purpose**: Call `com.atproto.sync.listRepos` on AT Protocol relays to get a paginated list of all DIDs and their PDS hosts.

**Input**: None (starts from cursor stored in Redis or 0)

**Output**: Writes to `repo_backfill` Redis stream
```json
{
  "did": "did:plc:abc123",
  "host": "https://pds.example.com",
  "rev": "revision-string",
  "status": "active",
  "active": true
}
```

**Configuration** (from docker-compose):
```yaml
INGESTER_RELAY_HOSTS: "relay1.us-east.bsky.network,relay1.us-west.bsky.network"
INGESTER_MODE: "all"  # or "backfill" to run only backfill
INGESTER_HIGH_WATER_MARK: "1000000"
INGESTER_BATCH_SIZE: "500"
INGESTER_BATCH_TIMEOUT_MS: "1000"
```

**Cursor Management**:
- Cursor stored in Redis key: `repo_backfill:cursor:{hostname}`
- Values: numeric string (e.g., "36724") or special marker "!ingester-done"
- When done, sets cursor to "!ingester-done" and sleeps for 5 minutes before rechecking

**Recent Fix Applied**:
- Added retry counter (max 5 consecutive errors)
- If max retries reached, skip ahead by incrementing cursor +1000
- This prevents infinite retry loops on problematic cursors
- Added detailed logging to track progress and errors

**Testing Plan**:
1. Reset cursors: `redis-cli SET "repo_backfill:cursor:relay1.us-east.bsky.network" "0"`
2. Run ingester in backfill mode
3. Monitor repo_backfill stream length: `redis-cli XLEN repo_backfill`
4. Check for increasing stream length and cursor progress logs

### 2. RepoBackfiller

**File**: `/Users/rudyfraser/Projects/rsky/rsky-backfiller/src/main.rs`

**Purpose**: Consume `repo_backfill` stream, call `com.atproto.sync.getRepo` for each DID at its PDS URL, and write the repo events to `firehose_backfill`.

**Input**: Consumes from `repo_backfill` Redis stream
```json
{
  "did": "did:plc:abc123",
  "host": "https://pds.example.com",
  "rev": "revision-string",
  "status": "active",
  "active": true
}
```

**Output**: Writes to `firehose_backfill` Redis stream
```json
{
  "type": "create",
  "seq": -1,  // Special value for backfill events
  "time": "2025-10-30T23:45:00Z",
  "did": "did:plc:abc123",
  "commit": "commit-cid",
  "rev": "revision-string",
  "collection": "app.bsky.feed.post",
  "rkey": "record-key",
  "cid": "record-cid",
  "record": { /* ... record data ... */ }
}
```

**Configuration** (from docker-compose):
```yaml
BACKFILLER_CONCURRENCY: "10"  # Number of concurrent getRepo requests
BACKFILLER_BATCH_SIZE: "50"
BACKFILLER_INPUT_STREAM: "repo_backfill"
BACKFILLER_OUTPUT_STREAM: "firehose_backfill"
BACKFILLER_CONSUMER_GROUP: "prod_backfiller"
BACKFILLER_CONSUMER_NAME: "prod_backfiller"
```

**Consumer Group Logic**:
- Uses XREADGROUP with consumer group "prod_backfiller"
- Reads batches of DIDs from `repo_backfill`
- For each DID, calls getRepo at the specified PDS host
- Parses CAR file and extracts all records
- Writes records to `firehose_backfill` stream
- Acknowledges processed messages with XACK

**Investigation Needed**:
- Check if RepoBackfiller is actually running (docker logs)
- Verify it's reading from repo_backfill (XINFO CONSUMERS)
- Check for errors in getRepo requests
- Monitor firehose_backfill stream length

**Testing Plan**:
1. Verify backfiller container is running: `docker ps | grep backfiller`
2. Check logs: `docker logs rust-backfiller --tail 100`
3. Check consumer group: `redis-cli XINFO CONSUMERS repo_backfill prod_backfiller`
4. Monitor streams:
   - `redis-cli XLEN repo_backfill` (should decrease or stay steady)
   - `redis-cli XLEN firehose_backfill` (should increase)

### 3. StreamIndexer

**File**: `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/main.rs`

**Purpose**: Consume events from `firehose_live`, `firehose_backfill`, and `label_live` streams and write to PostgreSQL.

**Status**: ✅ WORKING - Verified consuming from all streams and writing to PostgreSQL

**Configuration**:
```yaml
INDEXER_STREAMS: "firehose_live,firehose_backfill,label_live"
INDEXER_MODE: "stream"
INDEXER_GROUP: "firehose_group"
INDEXER_CONSUMER: "indexer1"
INDEXER_CONCURRENCY: "20"
INDEXER_BATCH_SIZE: "50"
```

## Testing Strategy

### Phase 1: Verify BackfillIngester Fix (Local)

1. Build updated ingester:
   ```bash
   cd ~/Projects/rsky
   cargo build --release --bin ingester
   ```

2. Reset backfill cursor to start fresh:
   ```bash
   redis-cli -h localhost -p 6380 SET "repo_backfill:cursor:relay1.us-east.bsky.network" "0"
   ```

3. Run BackfillIngester in backfill-only mode:
   ```bash
   RUST_LOG=info \
   REDIS_URL=redis://localhost:6380 \
   INGESTER_RELAY_HOSTS=relay1.us-east.bsky.network \
   INGESTER_MODE=backfill \
   INGESTER_HIGH_WATER_MARK=1000000 \
   ./target/release/ingester
   ```

4. Monitor in separate terminal:
   ```bash
   watch 'redis-cli -h localhost -p 6380 XLEN repo_backfill'
   ```

5. **Success Criteria**:
   - repo_backfill stream length INCREASES
   - Logs show "Write task wrote batch of X repos to Redis"
   - Logs show "Processed X repos, cursor: Y" every 10k repos
   - If 500 errors occur, logs show retry attempts and eventual skip-ahead

### Phase 2: Investigate RepoBackfiller

1. Check if backfiller is running:
   ```bash
   docker ps | grep backfiller
   docker logs rust-backfiller --tail 100
   ```

2. Check consumer group state:
   ```bash
   redis-cli -h localhost -p 6380 XINFO CONSUMERS repo_backfill prod_backfiller
   ```

3. Check stream lengths:
   ```bash
   redis-cli -h localhost -p 6380 XLEN repo_backfill
   redis-cli -h localhost -p 6380 XLEN firehose_backfill
   ```

4. **Expected Behavior**:
   - Backfiller should be reading from repo_backfill
   - firehose_backfill should be receiving new events
   - Consumer inactive time should be low (< 10 seconds)

5. **Possible Issues**:
   - Backfiller not running (container crashed)
   - Consumer group position wrong (same issue as indexers had)
   - getRepo requests failing (PDS unreachable, invalid DIDs)
   - Write task crashing silently (same issue as ingester had)

### Phase 3: End-to-End Verification

1. Monitor all stream lengths:
   ```bash
   while true; do
     clear
     echo "=== Stream Lengths ==="
     echo "repo_backfill:     $(redis-cli -h localhost -p 6380 XLEN repo_backfill)"
     echo "firehose_backfill: $(redis-cli -h localhost -p 6380 XLEN firehose_backfill)"
     echo "firehose_live:     $(redis-cli -h localhost -p 6380 XLEN firehose_live)"
     echo "label_live:        $(redis-cli -h localhost -p 6380 XLEN label_live)"
     sleep 5
   done
   ```

2. Check PostgreSQL record counts:
   ```sql
   SELECT
     (SELECT COUNT(*) FROM post) as posts,
     (SELECT COUNT(*) FROM actor) as actors,
     (SELECT COUNT(*) FROM feed_generator) as feed_gens;
   ```

3. **Success Criteria**:
   - repo_backfill: Should be increasing (BackfillIngester adding DIDs)
   - firehose_backfill: Should be increasing (RepoBackfiller processing repos)
   - PostgreSQL counts should be increasing (StreamIndexer writing records)
   - All consumer groups should show activity (low inactive times)

### Phase 4: Production Deployment

1. User pulls latest code and rebuilds:
   ```bash
   # On production server
   cd /mnt/nvme/bsky/atproto
   git pull origin rude1/backfill
   docker-compose -f docker-compose.prod-rust.yml build ingester
   docker-compose -f docker-compose.prod-rust.yml restart ingester
   ```

2. Monitor production streams (same as Phase 3)

3. If issues occur:
   - Check logs: `docker logs rust-ingester --tail 100`
   - Check environment variables: `docker inspect rust-ingester | grep -A 20 Env`
   - Compare with local working setup

## Key Learnings from Previous Fixes

1. **Consumer Group Position**: XREADGROUP cursor ">" only returns messages AFTER last-delivered-id
   - If last-delivered-id is ahead of all messages, consumer returns empty
   - Fix: `XGROUP SETID stream group 0` to reset position

2. **Silent Task Failures**: Async tasks that crash silently break the system
   - Always check `task.is_finished()` and log errors before aborting
   - Applied to FirehoseIngester (firehose.rs:192-215)
   - Applied to BackfillIngester (backfill.rs:215-232)

3. **Infinite Retry Loops**: Transient errors can cause permanent loops
   - Add retry counters with max limits
   - Implement skip-ahead or fallback strategies
   - Applied to BackfillIngester (backfill.rs:129-208)

4. **Backpressure**: High water marks prevent memory exhaustion
   - Check stream length before writing
   - Sleep and retry if stream too full
   - Currently set to 1,000,000 for all ingesters

## Next Steps

1. ✅ Apply BackfillIngester fix (DONE)
2. ⏳ Test BackfillIngester locally with production Redis/Postgres
3. ⏳ Investigate RepoBackfiller status
4. ⏳ Apply similar error handling fixes to RepoBackfiller if needed
5. ⏳ Deploy to production
6. ⏳ Monitor end-to-end backfill flow for 1 hour
7. ⏳ Verify PostgreSQL receiving backfilled records

## Files Modified

- `/Users/rudyfraser/Projects/rsky/rsky-ingester/src/backfill.rs`: Added retry logic and error recovery
- `/Users/rudyfraser/Projects/rsky/rsky-ingester/src/firehose.rs`: Added write task error logging
- `/Users/rudyfraser/Projects/rsky/CLAUDE.md`: Documented deployment and verification
- `/Users/rudyfraser/Projects/rsky/verify-indexing.sh`: Automated verification script

## Success Metrics

### Objective Criteria
- repo_backfill stream length INCREASING (not stuck at 850K)
- firehose_backfill stream length INCREASING
- PostgreSQL post count INCREASING
- All consumer groups showing activity (inactive < 10s)
- No error retry loops in logs

### Performance Targets
- BackfillIngester: Process 1000+ repos/batch
- RepoBackfiller: Process 50+ repos/batch (10 concurrent workers)
- StreamIndexer: Process 2000+ events/second
- End-to-end latency: Backfill events indexed within 1 minute

## Troubleshooting Guide

### Issue: repo_backfill not increasing

**Symptoms**:
- Stream length stays constant
- Logs show repeated errors for same cursor
- No "Processed X repos" messages

**Diagnosis**:
1. Check ingester logs for errors
2. Check cursor value in Redis
3. Try manual listRepos call to relay

**Solutions**:
- Reset cursor: `redis-cli SET "repo_backfill:cursor:relay1.us-east.bsky.network" "0"`
- Increase HIGH_WATER_MARK if backpressure active
- Check relay is reachable from container

### Issue: firehose_backfill not increasing

**Symptoms**:
- repo_backfill has messages but firehose_backfill doesn't grow
- Backfiller logs show errors or no activity

**Diagnosis**:
1. Check backfiller container: `docker ps | grep backfiller`
2. Check logs: `docker logs rust-backfiller`
3. Check consumer group: `XINFO CONSUMERS repo_backfill prod_backfiller`

**Solutions**:
- Restart backfiller container
- Reset consumer group position if needed
- Check PDS hosts are reachable
- Apply similar error handling fixes as ingester

### Issue: PostgreSQL not receiving backfilled records

**Symptoms**:
- firehose_backfill growing but PostgreSQL counts not increasing
- Indexer consuming but not writing

**Diagnosis**:
1. Check indexer logs for errors
2. Check which streams indexer is consuming
3. Verify indexer is processing firehose_backfill

**Solutions**:
- Ensure INDEXER_STREAMS includes "firehose_backfill"
- Reset indexer consumer group position if needed
- Check for database connection issues
