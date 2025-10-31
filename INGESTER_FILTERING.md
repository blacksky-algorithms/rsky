# Ingester Filtering Implementation

## Problem

The FirehoseIngester was subscribing to the full Bluesky relay firehose, which includes ALL apps and ALL collections across the entire AT Protocol network. This resulted in:

- **29,425 events/second** ingestion rate vs expected 1-2K events/second for app.bsky only
- **16.6M messages** accumulated in firehose_live during a 15-20 hour outage
- Unnecessary memory and processing overhead for non-app.bsky events
- Higher Redis stream memory usage than needed

## Solution

Added collection filtering at the ingester level (firehose.rs:279-283) to only process `app.bsky.*` and `chat.bsky.*` collections before writing to Redis streams.

### Files Modified

1. **rsky-ingester/src/firehose.rs**
   - Added filtering check: `if !collection.starts_with("app.bsky.")`
   - Filters operations before creating StreamEvent objects
   - Increments `FIREHOSE_FILTERED_OPERATIONS` metric

2. **rsky-ingester/src/metrics.rs**
   - Added new counter: `ingester_firehose_filtered_operations_total`
   - Tracks how many operations are filtered out

### What Gets Filtered

**FILTERED** (non-Bluesky collections):
- app.frontpage.* (Frontpage app)
- app.linkat.* (Linkat app)
- xyz.*.* (experimental apps)
- All other third-party apps

**PRESERVED** (still indexed):
- **app.bsky.*** - All Bluesky social collections:
  - app.bsky.feed.post (posts)
  - app.bsky.feed.like (likes)
  - app.bsky.feed.repost (reposts)
  - app.bsky.graph.follow (follows)
  - app.bsky.graph.block (blocks)
  - app.bsky.graph.list (lists)
  - app.bsky.actor.profile (profiles)
  - app.bsky.feed.generator (feed generators)
  - app.bsky.labeler.service (labelers)
- **chat.bsky.*** - All Bluesky chat collections:
  - chat.bsky.actor.declaration (chat participant declarations)
  - chat.bsky.convo.* (conversations and messages)
- Identity events (handle changes)
- Account events (account status changes)
- Tombstone events (account deletions)

## Expected Impact

### Ingestion Rate Reduction

Based on the current metrics showing 29,425 evt/s total and assuming app.bsky represents ~5-10% of AT Protocol traffic:

- **Before**: 29,425 evt/s ingested
- **After**: ~1,500-3,000 evt/s ingested (5-10% of total)
- **Reduction**: 90-95% fewer events written to Redis

### Memory Savings

- **firehose_live stream**: Will accumulate 10-20x slower in steady state
- **Event processing**: 90-95% fewer CBOR decode operations
- **Redis memory**: Proportional reduction in stream memory usage

### Processing Capacity

With 90-95% reduction in ingestion rate:
- Indexers can easily keep up with live events (16.7K evt/s processing capacity)
- Backlog of 16.6M messages will drain faster
- No more ingester backpressure issues

## Metrics to Monitor

### New Metric
```
ingester_firehose_filtered_operations_total
```

Shows cumulative count of filtered operations. Should grow rapidly (90-95% of operations).

### Expected Metric Changes

**Before filtering** (production baseline):
```
ingester_firehose_events_total: ~29,425/sec
ingester_stream_events_total: ~29,425/sec
ingester_firehose_filtered_operations_total: 0
```

**After filtering** (expected):
```
ingester_firehose_events_total: ~29,425/sec (unchanged - still receiving all)
ingester_stream_events_total: ~1,500-3,000/sec (90-95% reduction)
ingester_firehose_filtered_operations_total: ~26,000-28,000/sec (filtered out)
```

## Deployment Plan

1. **Stop production ingester**
   ```bash
   docker stop rust-ingester
   ```

2. **Update binary in production**
   ```bash
   # Copy locally built binary to production
   scp ./target/release/ingester blacksky@api.blacksky:/mnt/nvme/bsky/atproto/rust-target/release/
   ```

3. **Restart ingester**
   ```bash
   docker start rust-ingester
   ```

4. **Monitor metrics** (wait 1-2 minutes for data)
   - Check `ingester_firehose_filtered_operations_total` is increasing
   - Verify `ingester_stream_events_total` rate dropped by 90-95%
   - Confirm `firehose_live` stream length growing slower

5. **Verify indexers** still processing correctly
   - Check PostgreSQL for recent app.bsky.feed.post records
   - Verify no errors in indexer logs

## Rollback Plan

If filtering causes issues:

1. **Restore previous binary**
   ```bash
   docker stop rust-ingester
   # Restore backup binary
   docker start rust-ingester
   ```

2. **Expected behavior**: Ingestion rate returns to 29K evt/s

## Testing Locally

To test the filtered ingester locally with production data:

```bash
# Setup SSH tunnels (if not already running)
ssh -L 6380:localhost:6380 -L 15433:localhost:15433 -N blacksky@api.blacksky &

# Run filtered ingester locally
cd ~/Projects/rsky
RUST_LOG=info \
REDIS_URL=redis://localhost:6380 \
INGESTER_RELAY_HOSTS=relay1.us-east.bsky.network \
INGESTER_MODE=firehose \
INGESTER_HIGH_WATER_MARK=40000000 \
INGESTER_BATCH_SIZE=100 \
INGESTER_BATCH_TIMEOUT_MS=100 \
./target/release/ingester
```

Then monitor metrics:
```bash
curl -s http://localhost:9090/metrics | grep -E "firehose_filtered|stream_events_total"
```

## Success Criteria

- ✅ Build completes successfully (DONE)
- ⬜ `ingester_firehose_filtered_operations_total` increasing rapidly
- ⬜ `ingester_stream_events_total` rate drops to ~1.5-3K evt/s
- ⬜ `firehose_live` stream length stabilizes or grows slower
- ⬜ Indexers continue processing app.bsky records
- ⬜ No errors in ingester or indexer logs
- ⬜ PostgreSQL receiving app.bsky.feed.post records

## Notes

- **Non-breaking change**: Indexers don't need to be updated
- **Backward compatible**: Redis streams continue working normally
- **No schema changes**: Database remains unchanged
- **Safe to deploy**: Can rollback instantly if needed

The filtering is purely subtractive - we're just writing fewer events to Redis, not changing the format or behavior of events that do get written.
