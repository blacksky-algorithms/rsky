# CLAUDE.MD - Blacksky App View Indexing Pipeline

## 🎯 MISSION

**Get Blacksky App View to return same results as Bluesky App View for the Blacksky feed generator.**

### The Test

**Same request to feed generator, different App View backends:**

```bash
# Request to blackstar.quest feed generator using Bluesky App View
curl "https://blackstar.quest/xrpc/app.bsky.feed.getFeed?feed=at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky&limit=30" \
  -H "atproto-proxy: did:web:api.bsky.app#bsky_appview"
# Result: 30 posts ✅

# Same request using Blacksky App View
curl "https://blackstar.quest/xrpc/app.bsky.feed.getFeed?feed=at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky&limit=30" \
  -H "atproto-proxy: did:web:api.blacksky.community#bsky_appview"
# Result: {"feed":[], "cursor":"..."} ❌
```

### What We Know

1. ✅ **Feed generator works perfectly** - blackstar.quest is functioning correctly
2. ✅ **Both App Views use same API code** - Open source Bluesky dataplane
3. ❌ **Blacksky App View database is nearly empty** - Only 10,837 posts indexed today vs 4.1M expected (99.8% data loss)
4. ❌ **The problem is in MY code** - rsky-indexer, rsky-ingester, rsky-backfiller

### Architecture

```
Bluesky Firehose
     ↓
rsky-ingester → Redis Streams (firehose_live, firehose_backfill, labels)
     ↓
rsky-indexer (6 workers) → PostgreSQL
     ↓
Blacksky App View (dataplane) → Feed Generator
```

---

## 🔴 ROOT CAUSE: Expensive COUNT(*) Queries Blocking Indexing

### The Problem

**Symptom:** Only 10,837 posts indexed today vs 4.1M expected (99.8% data loss)

**Root Cause:** COUNT(*) queries in follow.rs and like.rs performing full table scans on EVERY event:

```rust
// follow.rs:165-173 - Runs on EVERY follow event
INSERT INTO profile_agg (did, "followersCount")
VALUES ($1, (SELECT COUNT(*) FROM follow WHERE "subjectDid" = $2))

// like.rs:150-156 - Runs on EVERY like event
INSERT INTO post_agg (uri, "likeCount")
VALUES ($1, (SELECT COUNT(*) FROM "like" WHERE subject = $2))
```

These COUNT(*) queries scan millions of rows on every event, causing:
1. Connection pool exhaustion (`Pool(Timeout(Wait))` errors)
2. 40M+ event lag on Redis streams
3. 99.8% of events never get indexed

### Evidence

**Database queries stuck on COUNT(*):**
```sql
-- As of 16:23 EST after 2 attempted fixes:
-- likeCount queries: 2 active (should be 0)
-- followersCount queries: 5 active (should be 0)
```

**Jetstream live test (2025-11-14 15:45 EST):**
```
Posts in 30 seconds: 1,427
Posts/second: 47.6
Projected posts/day: 4,109,642
ACTUAL DATABASE TODAY: 10,837 posts
MISSING: 99.8% data loss
```

**Redis stream lag:**
```
firehose_live:
  Stream length: 1,139,691 messages
  Consumer lag: ~40M events behind
  Pending: 216 messages stuck waiting for DB connections
```

---

## 🔧 FIXES APPLIED (2025-11-14 16:00-16:23 EST)

### Fix #1: Commented out followersCount queries (16:06 EST)
- File: `rsky-indexer/src/indexing/plugins/follow.rs`
- Lines: 163-178 (insert), 236-251 (delete)
- Committed: c50cdc2
- Deployed: Docker build + restart

### Fix #2: Commented out likeCount queries (16:18 EST)
- File: `rsky-indexer/src/indexing/plugins/like.rs`
- Lines: 148-162 (insert), 205-219 (delete)
- Committed: 30ffd23
- Deployed: Docker build + restart

### ⚠️ PROBLEM: Docker builds used cached layers

**After both deployments, COUNT queries still appearing in database!**

This means Docker cached old build layers and didn't recompile with the commented-out code.

**Current status:** Building with `--no-cache` flag to force full rebuild.

---

## 📋 DEPLOYMENT CHECKLIST

### Phase 1: Force Rebuild (IN PROGRESS)
```bash
# Force rebuild without cache
cd /mnt/nvme/bsky/atproto
docker compose -f docker-compose.prod-rust.yml build --no-cache indexer1

# Restart all indexers
docker compose -f docker-compose.prod-rust.yml stop indexer1 indexer2 indexer3 indexer4 indexer5 indexer6
docker compose -f docker-compose.prod-rust.yml up -d indexer1 indexer2 indexer3 indexer4 indexer5 indexer6
```

### Phase 2: Verify COUNT Queries Stopped
```bash
# Should return 0 for both
PGPASSWORD='...' psql -h localhost -p 15433 -U bsky -d bsky -c \
  "SELECT COUNT(*) FROM pg_stat_activity WHERE state = 'active' AND query LIKE '%SELECT COUNT(*) FROM%like%';"

PGPASSWORD='...' psql -h localhost -p 15433 -U bsky -d bsky -c \
  "SELECT COUNT(*) FROM pg_stat_activity WHERE state = 'active' AND query LIKE '%SELECT COUNT(*) FROM follow%';"
```

### Phase 3: Monitor Indexing Rate
```bash
# Posts should increase rapidly (target: ~47 posts/second = 4M/day)
watch -n 5 'PGPASSWORD="..." psql -h localhost -p 15433 -U bsky -d bsky -c \
  "SELECT COUNT(*) FROM post WHERE \"createdAt\" >= '\''2025-11-14 00:00:00'\'';"'
```

### Phase 4: Test Feed Parity
```bash
# Test Bluesky App View (should return 30 posts)
curl "https://blackstar.quest/xrpc/app.bsky.feed.getFeed?feed=at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky&limit=30" \
  -H "atproto-proxy: did:web:api.bsky.app#bsky_appview" | jq '.feed | length'

# Test Blacksky App View (should return ~30 posts when fixed)
curl "https://blackstar.quest/xrpc/app.bsky.feed.getFeed?feed=at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky&limit=30" \
  -H "atproto-proxy: did:web:api.blacksky.community#bsky_appview" | jq '.feed | length'
```

---

## 🎯 SUCCESS CRITERIA

**Mission complete when:**

1. ✅ COUNT(*) queries completely stopped (0 active in pg_stat_activity)
2. ✅ Posts indexed/day reaches ~4M (currently 10,837)
3. ✅ Pool(Timeout) errors stop completely
4. ✅ Redis stream lag decreases from 40M to <100K
5. ✅ **Blacksky App View returns same posts as Bluesky App View for Blacksky feed**

---

## 📊 MONITORING COMMANDS

### Check posts indexed today
```bash
ssh -p 2222 blacksky@api.blacksky.community \
  "PGPASSWORD='BEVoNPm7z0lT5tMAv6hF5SQUMkIQBTRHhx0JiKjxCsdVTR274zxdPw5o9CGtpmgh' psql -h localhost -p 15433 -U bsky -d bsky -c \"SELECT COUNT(*) FROM post WHERE \\\"createdAt\\\" >= '2025-11-14 00:00:00';\""
```

### Check COUNT queries
```bash
ssh -p 2222 blacksky@api.blacksky.community \
  "PGPASSWORD='BEVoNPm7z0lT5tMAv6hF5SQUMkIQBTRHhx0JiKjxCsdVTR274zxdPw5o9CGtpmgh' psql -h localhost -p 15433 -U bsky -d bsky -c \"SELECT query, wait_event FROM pg_stat_activity WHERE state = 'active' AND query LIKE '%COUNT%';\""
```

### Check Pool errors
```bash
ssh -p 2222 blacksky@api.blacksky.community \
  "docker logs rust-indexer1 2>&1 | grep 'Pool(Timeout)' | tail -10"
```

### Check Redis lag
```bash
redis-cli -p 6380 XINFO GROUPS firehose_live | grep -E "lag|pending"
```

---

## 🔍 CRITICAL ARCHITECTURE QUESTIONS

**Why is indexing so slow?**

1. ✅ **IDENTIFIED:** COUNT(*) queries blocking connection pool
2. ⏳ **IN PROGRESS:** Deploying fix to stop COUNT queries
3. ❓ **TO INVESTIGATE:** Are there other slow queries after COUNT fix?
4. ❓ **TO INVESTIGATE:** Is connection pool size adequate? (currently 20 per indexer)
5. ❓ **TO INVESTIGATE:** Are indexers processing events in optimal batch sizes?

**What queries do indexers actually need?**

A feed generator needs to hydrate:
- Posts (author, content, images, etc)
- Likes (who liked what)
- Reposts (who reposted what)
- Follows (who follows who)
- Profiles (display names, avatars)

All of these are simple INSERT/UPDATE/DELETE operations. COUNT(*) aggregates are NOT required for feed generation.

---

## 🐛 REMAINING ISSUES TO INVESTIGATE

### After COUNT Fix Deploys

1. **Verify indexing rate increases**
   - Current: 10,837 posts/day (0.125 posts/sec)
   - Expected: 4.1M posts/day (47.6 posts/sec)
   - Need: 380x improvement

2. **Check for other slow queries**
   - Are there other expensive operations?
   - Check pg_stat_statements for slow queries

3. **Verify feed parity**
   - Test exact same request to both app views
   - Compare JSON responses
   - Ensure Blacksky returns same posts as Bluesky

4. **Monitor connection pool**
   - Are 20 connections per indexer enough?
   - Should we increase pool size?
   - Or reduce query complexity?

---

## 🧪 TESTING PLAN

### Test 1: Jetstream Live Rate (Baseline)
```bash
cd /tmp
node jetstream_test.js
# Expected: 47.6 posts/sec = 4.1M posts/day
```

### Test 2: Feed Parity (THE MISSION)
```bash
# Bluesky App View
curl -s "https://blackstar.quest/xrpc/app.bsky.feed.getFeed?feed=at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky&limit=30" \
  -H "atproto-proxy: did:web:api.bsky.app#bsky_appview" \
  > /tmp/bluesky_feed.json

# Blacksky App View
curl -s "https://blackstar.quest/xrpc/app.bsky.feed.getFeed?feed=at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky&limit=30" \
  -H "atproto-proxy: did:web:api.blacksky.community#bsky_appview" \
  > /tmp/blacksky_feed.json

# Compare
echo "Bluesky posts: $(jq '.feed | length' /tmp/bluesky_feed.json)"
echo "Blacksky posts: $(jq '.feed | length' /tmp/blacksky_feed.json)"

# SUCCESS when both return ~30 posts
```

### Test 3: Database Query Performance
```bash
# After fix, verify no slow COUNT queries
PGPASSWORD='...' psql -h localhost -p 15433 -U bsky -d bsky -c \
  "SELECT query, calls, mean_exec_time FROM pg_stat_statements
   WHERE query LIKE '%COUNT%'
   ORDER BY mean_exec_time DESC LIMIT 10;"
```

---

## 📝 CODE TO REVIEW

### Files Modified
1. `rsky-indexer/src/indexing/plugins/follow.rs`
   - Lines 163-178: followersCount INSERT (commented out)
   - Lines 236-251: followersCount DELETE (commented out)
   - Lines 182-189: followsCount INSERT (kept - has coalesce locking)
   - Lines 254-262: followsCount DELETE (kept - has coalesce locking)

2. `rsky-indexer/src/indexing/plugins/like.rs`
   - Lines 148-162: likeCount INSERT (commented out)
   - Lines 205-219: likeCount DELETE (commented out)

3. `rsky-indexer/src/indexing/plugins/post.rs`
   - Lines ???: replyCount queries (TO INVESTIGATE - may also be slow)
   - Lines ???: postsCount queries (has coalesce locking - should be OK)

---

## 🔗 RESOURCES

**SSH:** `ssh -p 2222 blacksky@api.blacksky.community`
**Sudo password:** `$yRdtLwaV*9u9*&D`
**Redis:** `redis-cli -p 6380` (localhost via tunnel)
**Database:** `psql -h localhost -p 15433 -U bsky -d bsky`

**Test endpoints:**
- Bluesky App View: `did:web:api.bsky.app#bsky_appview`
- Blacksky App View: `did:web:api.blacksky.community#bsky_appview`
- Feed Generator: `https://blackstar.quest/xrpc/app.bsky.feed.getFeed`

**Jetstream firehose:**
```bash
wscat -c 'wss://jetstream2.us-west.bsky.network/subscribe?wantedCollections=app.bsky.feed.post'
```

---

**Last updated: 2025-11-14 16:25 EST**
**Status: Rebuilding with --no-cache to deploy COUNT fix**
- Add robust logging. Always build and test the code locally using the locally running ssh tunnels for redis and postgres before committing to git and trying to build the docker files in production. If it's not working and producing the correct outputs locally it won't work in production.
- When testing locally -- that means on this machine (/Users/rudyfraser/Projects/rsky -- rudyfraser@MacBook-Pro-3 rsky % whoami
rudyfraser) NOT blacksky@api:/mnt/nvme/bsky/atproto$ whoami
blacksky
blacksky@api:/mnt/nvme/bsky/atproto$ pwd
/mnt/nvme/bsky/atproto
blacksky@api:/mnt/nvme/bsky/atproto$  -- that's production which is reached via ssh)