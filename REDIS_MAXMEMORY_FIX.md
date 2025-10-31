# Redis Maxmemory Fix (2025-10-31)

## Problem

Redis was hitting OOM errors during backfill:
```
ERROR: Redis(OOM: command not allowed when used memory > 'maxmemory'.)
```

## Root Cause

- Redis maxmemory was set to **32GB** (34359738368 bytes)
- Redis was using **33.1GB** (exceeded limit)
- Streams filled with unfiltered data (all collections, not just Bluesky)

## Solution Applied

Increased Redis maxmemory to **100GB** via SSH tunnel:
```bash
redis-cli -h localhost -p 6380 CONFIG SET maxmemory 107374182400
```

## Current Stream State (After Filtering Deployed)

```
firehose_backfill: 42,539,339 messages (42.5M)
firehose_live:     16,782,054 messages (16.8M)
repo_backfill:       841,134 messages (841K)
```

**Note**: These streams contain OLD unfiltered data (app.frontpage, app.linkat, etc.) from before filtering was deployed. The filtered ingester/backfiller will prevent future growth with non-Bluesky records.

## To Make Permanent

The maxmemory change is **active now** but won't survive a Redis restart. Add to docker-compose.prod-rust.yml:

```yaml
services:
  backfill-redis:
    image: redis:7-alpine
    command: redis-server --maxmemory 107374182400 --maxmemory-policy allkeys-lru
```

Or create a redis.conf file and mount it:
```
maxmemory 107374182400
maxmemory-policy allkeys-lru
```

## Expected Behavior Going Forward

1. **No more OOM errors** - 100GB headroom vs 33GB usage
2. **Slower growth** - Filtered backfiller writes 90-95% fewer records
3. **Natural drainage** - Indexers will gradually process old unfiltered data
4. **Steady state** - Once old data is drained, Redis usage should stabilize at ~5-10GB

## Verification Commands

Check Redis memory usage:
```bash
redis-cli -h localhost -p 6380 INFO memory | grep used_memory_human
redis-cli -h localhost -p 6380 CONFIG GET maxmemory
```

Check stream lengths:
```bash
redis-cli -h localhost -p 6380 XLEN firehose_backfill
redis-cli -h localhost -p 6380 XLEN firehose_live
redis-cli -h localhost -p 6380 XLEN repo_backfill
```

Monitor filtering metrics:
```bash
# Backfiller filtering
curl -s http://localhost:9091/metrics | grep backfiller_records_filtered_total
curl -s http://localhost:9092/metrics | grep backfiller_records_filtered_total

# Ingester filtering
curl -s http://localhost:4100/metrics | grep ingester_firehose_filtered_operations_total
```

## Timeline

- **Before**: 32GB limit, 33.1GB usage → OOM errors
- **After**: 100GB limit, 33.1GB usage → No OOM
- **Future**: 100GB limit, ~5-10GB usage (after old data drains)
