# Production Deployment Status

## Current Status: Partially Working ✅⚠️

The Rust ingester is running in production with the following status:

### ✅ Working Components

1. **Firehose Connections** - Both relays connected successfully
   - relay1.us-east: cursor=6322875401 (correct large number!)
   - relay1.us-west: cursor=6002880394 (correct large number!)

2. **CBOR Error Handling** - Malformed messages are being handled gracefully
   - `Failed to decode #commit message: Mismatch { expect_major: 3, byte: 216 }`
   - These are logged but don't crash the ingester ✅

3. **Container Stability** - Container is running (though restarting periodically)

### ⚠️ Issues Found & Fixed

#### 1. Labeler Cursor Bug (Fixed in commit a65ab25)
**Problem:** Labeler cursor was showing `cursor=1` after restart instead of large sequence number

**Root Cause:** Same bug as firehose - using `header.operation` (always 1) instead of `body.seq`

**Fix Applied:**
- Changed line 153 in `rsky-ingester/src/labeler.rs`
- Now uses `body.seq` instead of `header.operation as i64`

**To Deploy:** See "Deployment Steps" below

#### 2. Backfill Errors (Expected, Not a Bug)
The backfill is showing 500 errors from the relay:
```
BackfillIngester error: listRepos failed: 500 Internal Server Error
{"error":"failed to get repo root for (did:plc:r47ixofywwpyonnqd5d4pmaz): repository state not available"}
```

**Status:** These are **server-side errors** from the Bluesky relay, not bugs in our code. The backfiller correctly retries these errors. This is expected behavior.

#### 3. Container Restarts
The container restarted at 23:00:19 (18 seconds after initial start). This might be related to:
- The labeler cursor=1 bug (now fixed)
- Backpressure/resource issues
- Need to monitor after deploying labeler fix

## Deployment Steps for Labeler Fix

On the production server:

```bash
cd /mnt/nvme/bsky/rsky

# Pull latest changes
git pull origin rude1/backfill

# Rebuild ingester image
docker build --no-cache -t rsky-ingester:latest -f rsky-ingester/Dockerfile .

# Clear the bad labeler cursor
docker exec backfill-redis redis-cli DEL "label_live:cursor:atproto.africa"

# Restart ingester
docker compose -f /mnt/nvme/bsky/atproto/docker-compose.prod-rust.yml restart ingester

# Monitor logs
docker logs -f rust-ingester
```

## What to Look For After Deployment

### Good Signs ✅
- Labeler cursor shows large number (not 1)
- Firehose cursors continue to update with large numbers
- Container stays running for >5 minutes
- Events flowing into Redis streams

### Check Commands

```bash
# Check cursors in Redis
docker exec backfill-redis redis-cli MGET \
  "firehose_live:cursor:relay1.us-east.bsky.network" \
  "firehose_live:cursor:relay1.us-west.bsky.network" \
  "label_live:cursor:atproto.africa"

# Check stream lengths
docker exec backfill-redis redis-cli XLEN firehose_live
docker exec backfill-redis redis-cli XLEN label_live
docker exec backfill-redis redis-cli XLEN repo_backfill

# Check if container is restarting
docker ps | grep rust-ingester
# Look at "STATUS" column - should say "Up X minutes" not "Up X seconds"
```

## Summary of All Fixes

### Commit History
1. **fe16a80** - Handle AT Protocol Sync v1.1 #sync and #info messages
2. **b95af27** - Gracefully handle CBOR decoding errors in firehose messages
3. **107d69d** - Clarify cursor semantics with correct defaults
4. **b6b44b4** - Fix cursor=1 bug for firehose (seq field)
5. **a65ab25** - Fix cursor=1 bug for labeler (seq field) ← **NEED TO DEPLOY**

### Files Modified (Total)
- `rsky-firehose/src/firehose.rs` - #sync/#info handling, CBOR error handling
- `rsky-ingester/src/firehose.rs` - Fixed seq usage (commit.seq not header.operation)
- `rsky-ingester/src/labeler.rs` - Fixed seq usage (body.seq not header.operation)
- `rsky-ingester/src/bin/ingester.rs` - Fixed "all" mode crash when no labelers
- `rsky-ingester/Dockerfile` - Use nightly Rust
- `rsky-indexer/Dockerfile` - Use nightly Rust
- `rsky-backfiller/Dockerfile` - Use nightly Rust

## Expected Behavior After Full Deployment

Once the labeler fix is deployed:

1. **All cursors should show large numbers**
   - Firehose: 6+ billion range
   - Labeler: 12+ million range

2. **Container should stay running**
   - No restarts due to cursor=1 issues

3. **Events should flow continuously**
   - firehose_live stream growing
   - label_live stream growing (if labeler has events)
   - repo_backfill stream growing

4. **Errors should be minimal**
   - Occasional CBOR decode warnings (handled gracefully)
   - Occasional backfill 500 errors (server-side, retried)
   - No crashes

## Next Steps

1. **Deploy labeler fix** (commands above)
2. **Monitor for 15-30 minutes** to ensure stability
3. **Check Redis streams** to verify events are flowing
4. **If stable**, consider enabling indexer and backfiller Rust services

## Questions to Answer

- [ ] Is the labeler cursor now a large number after restart?
- [ ] Is the container staying up for >15 minutes?
- [ ] Are events flowing into Redis streams?
- [ ] Are there any new errors in the logs?
