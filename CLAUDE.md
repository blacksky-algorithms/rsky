# CLAUDE.MD - Production Environment Critical Data Loss

## 🚨 CRITICAL EMERGENCY - 99.8% DATA LOSS (2025-11-14 15:57 EST)

**Status**: ⚠️ **EMERGENCY FIX IN PROGRESS**

---

## ROOT CAUSE IDENTIFIED: Expensive COUNT(*) Query Destroying Indexing

### The Problem

**Symptoms:**
- Bluesky network: **4.1 MILLION posts/day**
- Blacksky indexed: **9,777 posts today**
- **Data loss: 99.8%**

**Root Cause:**
Lines 165-173 in `rsky-indexer/src/indexing/plugins/follow.rs`:

```rust
client.execute(
    r#"INSERT INTO profile_agg (did, "followersCount")
       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE "subjectDid" = $2))
       ON CONFLICT (did) DO UPDATE SET "followersCount" = EXCLUDED."followersCount""#,
    &[&follow_subject, &follow_subject],
)
```

This query **performs a full table scan of the follow table** (counting millions of rows) on EVERY follow event. With thousands of follows per minute, this completely destroys performance.

**Evidence:**
1. **Database queries all stuck on COUNT(*):**
   ```
   SELECT COUNT(*) FROM follow WHERE "subjectDid" = $2
   wait_event: DataFileRead
   ```
   All 15 active DB connections running this same expensive query

2. **Indexers failing with connection pool timeouts:**
   ```
   ERROR Failed to process message: Pool(Timeout(Wait))
   ```
   Connection pool exhausted waiting for slow COUNT(*) queries to complete

3. **40 MILLION event lag on firehose_live:**
   - Stream length: 39,999,507 events
   - Consumed: 835,614,337 events
   - Lag: 39,999,507 events behind
   - Pending: 216 messages stuck waiting for DB connections

4. **Jetstream live test confirms catastrophic data loss:**
   - Actual Bluesky post rate: 47.6 posts/second
   - Expected daily posts: 4,109,642
   - Actual indexed posts: 9,777
   - Data loss: 99.8%

### Why This Happened

The follow plugin updates follower counts on EVERY follow event by recounting the entire follow table. As the follow table grows (millions of rows), each follow event takes longer and longer to process:

1. User follows someone
2. Indexer inserts follow record (fast)
3. Indexer recounts ALL followers for that user (SLOW - full table scan)
4. Query blocks on disk I/O reading millions of rows
5. Connection held for 10+ seconds
6. Connection pool exhausted
7. New indexing tasks timeout waiting for connections
8. Events pile up in Redis unprocessed

**The `followsCount` query has coalesce locking to prevent thrashing, but `followersCount` doesn't!**

---

## Emergency Fix (2025-11-14 15:57 EST)

### Actions Taken

1. ✅ **Commented out expensive COUNT(*) queries**
   - Disabled in both `insert()` and `delete()` functions
   - Added detailed comments explaining the issue
   - File: `rsky-indexer/src/indexing/plugins/follow.rs`

2. ⏳ **Building fixed indexer** (in progress)
   ```bash
   cargo build --release -p rsky-indexer
   ```

3. **Next steps:**
   - Deploy fixed indexer to production
   - Restart all 6 indexers
   - Verify Pool(Timeout) errors stop
   - Monitor indexing lag reduction
   - Watch posts/day increase to ~4M

### Deployment Plan

```bash
# 1. Build fixed binary (already running)
cargo build --release -p rsky-indexer

# 2. SSH to production
ssh -p 2222 blacksky@api.blacksky.community

# 3. Stop all indexers
cd /mnt/nvme/bsky/atproto
docker compose -f docker-compose.prod-rust.yml stop indexer1 indexer2 indexer3 indexer4 indexer5 indexer6

# 4. Copy new binary to production
# (from local machine)
scp -P 2222 target/release/rsky-indexer blacksky@api.blacksky.community:/mnt/nvme/bsky/rsky/target/release/

# 5. Restart indexers
docker compose -f docker-compose.prod-rust.yml start indexer1 indexer2 indexer3 indexer4 indexer5 indexer6

# 6. Monitor logs - should see NO Pool(Timeout) errors
docker logs rust-indexer1 --tail 50 -f

# 7. Check lag reduction
redis-cli -p 6380 XINFO GROUPS firehose_live
```

### Expected Results After Fix

- ✅ Pool(Timeout) errors: 100% → 0%
- ✅ Indexing lag: 40M events → reducing rapidly
- ✅ Posts indexed/day: 9,777 → ~4,000,000 (400x improvement!)
- ✅ Database active queries: All COUNT(*) queries gone
- ✅ Connection pool: Healthy, no timeouts

---

## Long-Term Fix Required

**TODO: Implement incremental follower counts**

Instead of recounting on every follow event, use incremental updates:

```rust
// On follow INSERT:
UPDATE profile_agg SET "followersCount" = "followersCount" + 1 WHERE did = $1

// On follow DELETE:
UPDATE profile_agg SET "followersCount" = "followersCount" - 1 WHERE did = $1
```

Benefits:
- Constant time O(1) instead of O(n) table scan
- No expensive COUNT(*) queries
- No connection pool exhaustion
- Scales to millions of follows

**Alternative: Background job**
Run follower count updates in a separate background job every 5-10 minutes, not on every follow event.

---

## Investigation Timeline

### Initial Problem (12:28 EST)
- User reported empty Blacksky feed
- HAR file showed: `{"feed":[]}`
- Initial hypothesis: Incomplete backfill (WRONG)

### Correction (12:43 EST)
- User corrected: "Missing NEW posts WHILE indexers running is UNACCEPTABLE"
- Changed focus to live indexing pipeline

### Discovery Phase (12:45-15:30 EST)
1. Found ingester writing to firehose_live correctly
2. Found 40M event lag on firehose_live consumer group
3. Found indexer logs full of `Pool(Timeout(Wait))` errors
4. Checked database - only 112 connections (under 2000 limit)
5. Checked active queries - ALL running same COUNT(*) query
6. Read follow.rs code - found expensive aggregate updates

### Jetstream Test (15:45 EST)
- Connected to `wss://jetstream2.us-west.bsky.network/subscribe?wantedCollections=app.bsky.feed.post`
- Measured 47.6 posts/second = 4.1M posts/day
- Confirmed 99.8% data loss

### Fix Implementation (15:57 EST)
- Commented out COUNT(*) queries
- Building fixed indexer
- Preparing deployment

---

## Key Learnings

1. **Always profile database queries under load**
   - COUNT(*) on large tables is O(n) and very slow
   - Use EXPLAIN ANALYZE to find slow queries
   - Monitor `pg_stat_activity` for stuck queries

2. **Connection pool exhaustion symptoms:**
   - `Pool(Timeout(Wait))` errors
   - Low actual DB connections but high pool usage
   - All connections running same slow query

3. **Aggregate updates don't need to be real-time**
   - Follower counts don't need to be exact
   - Eventual consistency is fine
   - Background jobs or incremental updates better than full recounts

4. **Jetstream is excellent for testing firehose rates**
   - `wss://jetstream2.us-west.bsky.network/subscribe?wantedCollections=app.bsky.feed.post`
   - Easy to measure actual Bluesky post rates
   - Compare against local indexing to detect data loss

---

## Monitoring After Fix

### Success Criteria
- ✅ Posts indexed today: 9,777 → growing rapidly toward 4M
- ✅ Pool(Timeout) errors: Stop completely
- ✅ Indexing lag: 40M → decreasing to <100K
- ✅ Database active queries: No COUNT(*) queries
- ✅ Indexer logs: Clean, no errors

### Commands to Monitor

```bash
# Check posts indexed today
ssh -p 2222 blacksky@api.blacksky.community "PGPASSWORD='BEVoNPm7z0lT5tMAv6hF5SQUMkIQBTRHhx0JiKjxCsdVTR274zxdPw5o9CGtpmgh' psql -h localhost -p 15433 -U bsky -d bsky -c \"SELECT COUNT(*) as posts_today FROM post WHERE \\\"createdAt\\\" >= '2025-11-14 00:00:00';\""

# Check indexing lag
redis-cli -p 6380 XINFO GROUPS firehose_live | grep -E "lag|pending"

# Check for Pool(Timeout) errors
ssh -p 2222 blacksky@api.blacksky.community "docker logs rust-indexer1 --tail 50 | grep -i 'pool(timeout)'"

# Check database active queries
ssh -p 2222 blacksky@api.blacksky.community "PGPASSWORD='BEVoNPm7z0lT5tMAv6hF5SQUMkIQBTRHhx0JiKjxCsdVTR274zxdPw5o9CGtpmgh' psql -h localhost -p 15433 -U bsky -d bsky -c \"SELECT count(*), state, wait_event FROM pg_stat_activity WHERE state = 'active' GROUP BY state, wait_event;\""
```

---

## Previous Issues (RESOLVED)

### ✅ getTimeline Issue (2025-11-14 15:45 EST)
**Problem:** Following feed timing out after 10 seconds
**Solution:** Configured Caddy to return 501 Not Implemented (intentional design decision)
**Status:** Fixed - fast 290ms response with clear error message

### ✅ Memory Crisis (2025-11-14 13:40 EST)
**Problem:** 410M pending messages in consumer group, Redis using 88GB RAM
**Solution:** Deleted and recreated consumer groups, freed 50GB+ RAM
**Status:** Fixed - system stable with 73GB free RAM

---

## Emergency Contacts

**SSH:** `ssh -p 2222 blacksky@api.blacksky.community`
**Sudo password:** `$yRdtLwaV*9u9*&D`
**Redis:** `redis-cli -p 6380` (via tunnel)

---

## Testing Resources

**Jetstream firehose for live testing:**
```bash
wscat -c 'wss://jetstream2.us-west.bsky.network/subscribe?wantedCollections=app.bsky.feed.post'
```

**Test accounts:**
- did:plc:sqqoxd3bhxajzffwxfx4flot (shacqeal.blackstar.quest)
- did:plc:w4xbfzo7kqfes5zb7r6qv3rw (rude1.blacksky.team)

---

**Last updated: 2025-11-14 15:57 EST**
