# XTRIM Fix Report - 2025-10-31

## Problem Analysis

XTRIM was deployed but streams are NOT decreasing. Root cause identified:

### Issue 1: Unreachable Trim Code Paths
**Original Bug** (rsky-indexer/src/stream_indexer.rs):
- Lines 120-128: Early return when `messages.is_empty()` prevented trim code from executing
- Lines 219-227: Condition `if *cursor != ">"` caused working indexers to skip trim

**Fix Applied**:
- Added trim to empty-message path (lines 127-133)
- Removed cursor condition so trim runs for ALL indexers (lines 225-234)

### Issue 2: Phantom Pending Messages Blocking Trim
**Current Production State**:
```
Stream first entry:     1761930950204-409
Consumer last-delivered: 1761930942534-312 (BEFORE stream start!)
Oldest pending message:  1760728859999-84  (rust-indexer4, doesn't exist in stream!)
Stream length:          59.7M messages
```

**Why XTRIM Returns 0**:
1. Code calls: `XTRIM firehose_backfill MINID 1761930942534-312`
2. Redis checks: "Keep only messages >= 1761930942534-312"
3. Stream already starts at `1761930950204-409` (later than threshold)
4. Result: 0 messages trimmed

**Why This Happened**:
- Stuck indexers (rust-indexer4, rust-indexer5, rust-indexer6) crashed or were restarted
- They hold pending messages from OLD cursors (e.g., `1760728859999`)
- Those messages were already trimmed by production backfiller
- Redis still tracks them as "pending" even though they don't exist
- This creates 299 phantom pending messages that can never be ACKed

## Solution

### Part 1: Code Fix (COMPLETED ✅)
**Files Modified**:
- `rsky-indexer/src/stream_indexer.rs` - Fixed unreachable trim paths
- `rsky-indexer/src/consumer.rs` - Added trim_stream() and get_group_cursor() methods
- `rsky-backfiller/src/repo_backfiller.rs` - Added trim functionality

**Build Command**:
```bash
cd ~/Projects/rsky
cargo build --release --bin indexer
cargo build --release --bin backfiller
```

### Part 2: Production Deployment (REQUIRED BEFORE XTRIM WORKS)

**Step 1: Clean Up Stuck Consumers**

Production has 3 stuck consumers with phantom pending messages:
- rust-indexer4: cursor ahead of stream, getting 0 messages
- rust-indexer5: cursor ahead of stream, getting 0 messages
- rust-indexer6: consuming from wrong stream (label_live)

**Delete these consumers** (this clears their phantom pending messages):
```bash
redis-cli -h localhost -p 6380 XGROUP DELCONSUMER firehose_backfill firehose_group rust-indexer4
redis-cli -h localhost -p 6380 XGROUP DELCONSUMER firehose_backfill firehose_group rust-indexer5
redis-cli -h localhost -p 6380 XGROUP DELCONSUMER label_live label_group rust-indexer6
```

**Step 2: Deploy New Binaries**

Copy updated binaries to production:
```bash
# On local machine (from ~/Projects/rsky)
scp target/release/indexer blacksky@api.blacksky:/mnt/nvme/bsky/atproto/rust-target/release/
scp target/release/backfiller blacksky@api.blacksky:/mnt/nvme/bsky/atproto/rust-target/release/
```

**Step 3: Update docker-compose.prod-rust.yml**

Fix indexer6 configuration (currently pointing to label_live):
```yaml
rust-indexer6:
  environment:
    - INDEXER_STREAMS=firehose_live,firehose_backfill  # NOT label_live!
    - INDEXER_GROUP=firehose_group
    - INDEXER_CONSUMER=rust-indexer6
```

**Step 4: Restart Indexers**

```bash
docker compose -f docker-compose.prod-rust.yml restart rust-indexer4 rust-indexer5 rust-indexer6
```

**Step 5: Restart Backfillers** (optional, to get trim functionality)

```bash
docker compose -f docker-compose.prod-rust.yml restart rust-backfiller1 rust-backfiller2
```

### Part 3: Verification

**Check 1: All indexers consuming**
```bash
# Should show inactive time < 10 seconds for all 6 indexers
redis-cli -h localhost -p 6380 XINFO CONSUMERS firehose_backfill firehose_group | grep -E "name|inactive"
```

**Check 2: Streams decreasing**
```bash
# Run multiple times, 30 seconds apart - should see length decreasing
redis-cli -h localhost -p 6380 XLEN firehose_backfill
redis-cli -h localhost -p 6380 XLEN firehose_live
```

**Check 3: Trim logs appearing**
```bash
# Should see "Trimmed X messages" every few seconds
docker logs --tail 100 rust-indexer1 | grep Trimmed
docker logs --tail 100 rust-indexer2 | grep Trimmed
```

**Check 4: No phantom pending messages**
```bash
# Should show 0 or small number with recent timestamps
redis-cli -h localhost -p 6380 XPENDING firehose_backfill firehose_group
```

## Expected Behavior After Fix

**Before**:
- firehose_backfill: 59.7M messages (not decreasing)
- firehose_live: 1.45M messages (not decreasing)
- 3 of 6 indexers stuck/misconfigured
- No "Trimmed X messages" logs
- Backfiller backpressure errors

**After**:
- All 6 indexers actively consuming (inactive < 10 sec)
- Streams visibly decreasing (thousands/second)
- Frequent "Trimmed X messages" logs
- Backfillers resume processing repo_backfill
- Redis memory usage stabilizing

## Why XTRIM Didn't Work Before

1. **Code bugs**: Unreachable trim paths meant trim never executed
2. **Phantom pending messages**: Stuck consumers holding messages that don't exist
3. **Cursor misalignment**: Consumer positions ahead of or behind stream content

All three issues must be fixed for XTRIM to function properly.

## Technical Details

### XTRIM Strategy
We use `XTRIM MINID` with the consumer group's `last-delivered-id`:
```rust
let group_cursor = self.consumer.get_group_cursor().await?;
self.consumer.trim_stream(&group_cursor).await?;
```

This is safe because:
- `last-delivered-id` advances when messages are claimed by consumers
- Messages before this ID have been delivered to consumers
- Once ACKed and deleted, they can be safely trimmed

### Why We Use last-delivered-id Not Oldest Pending
- Oldest pending message may not exist (phantom messages)
- last-delivered-id is always valid (maintained by Redis)
- XTRIM with MINID is idempotent (safe to run multiple times)

### Trim Frequency
Trim runs after EVERY batch of messages:
- Empty batch: Trim in idle loop (lines 127-133)
- Successful batch: Trim after processing (lines 225-234)

This ensures constant memory cleanup as messages are consumed.

## Next Steps

1. **Deploy fix immediately** (all 3 parts of solution)
2. **Monitor for 1 hour** to verify streams decreasing
3. **Move to Phase 2** (fix remaining indexer issues) from CLAUDE.md
4. **Move to Phase 3** (resolve backfiller backpressure)
5. **Move to Phase 4** (fix dashboard metrics)
