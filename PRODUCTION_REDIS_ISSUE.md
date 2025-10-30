# 🚨 CRITICAL: Production Redis Out of Memory

**Discovered**: 2025-10-30 during indexer testing
**Severity**: CRITICAL - Blocking all queue processing
**Status**: ACTIVE INCIDENT

## Issue Summary

Production Redis has **completely filled its memory allocation** and is refusing all write operations, including message acknowledgments. This is why the queues are backed up with 50M+ messages.

## Symptoms

```
ERROR: Redis(OOM: command not allowed when used memory > 'maxmemory'.)
```

## Current State

```
Redis Memory Usage:
  Used Memory: 32.00 GB
  Max Memory:  32.00 GB
  Usage:       100% (FULL!)
  Policy:      noeviction (refuses all writes when full)

Queue Depths:
  firehose_live:     19,629,405 messages
  firehose_backfill: 30,532,997 messages
  TOTAL:             50,162,402 messages

Consumer Group Status:
  Group:    firehose_group
  Consumers: 17 active
  Pending:   2,896,817 messages (assigned but not ACKed)
  Lag:       16,728,817 messages (queue growing faster than consumed)
```

## Root Cause

1. **Memory Full**: Redis has reached its 32GB `maxmemory` limit
2. **No Eviction**: `maxmemory-policy: noeviction` means Redis refuses ALL writes when full
3. **Cannot ACK**: Indexers cannot acknowledge processed messages (ACK is a write operation)
4. **Queue Buildup**: Messages pile up because:
   - Ingester keeps adding new messages (as long as it can)
   - Indexers process but can't ACK, so messages stay in pending
   - Pending messages consume memory
   - Eventually all writes fail

## Impact

- ✅ **Indexers CAN read** messages (XREADGROUP still works)
- ❌ **Indexers CANNOT ACK** messages (XACK requires write)
- ❌ **Ingester CANNOT add** new messages (XADD requires write)
- ❌ **NO new events** being ingested since Redis filled
- ⚠️ **Data processing stalled** - events being missed

## Why This Happened

Looking at your `docker-compose.prod.yml`:

```yaml
redis:
  command: redis-server --appendonly yes --appendfsync everysec --maxmemory 32gb --maxmemory-policy noeviction
```

The `noeviction` policy is typically used when you want Redis to act as a strict cache and never lose data. However, for a **message queue** use case, this is problematic because:

1. Message queues naturally accumulate data
2. Stream messages include:
   - Message content
   - Consumer group tracking
   - Pending entry lists (PEL) for unacknowledged messages
3. With millions of messages, 32GB fills quickly

## Immediate Solutions

### Option 1: Increase Memory (Quick Fix)
```yaml
redis:
  command: redis-server --appendonly yes --appendfsync everysec --maxmemory 64gb --maxmemory-policy noeviction
```

Then restart Redis. **WARNING**: This may cause downtime and message loss.

### Option 2: Change Eviction Policy (Risky)
```yaml
redis:
  command: redis-server --appendonly yes --appendfsync everysec --maxmemory 32gb --maxmemory-policy allkeys-lru
```

This allows Redis to evict old messages when full. **WARNING**: May lose unprocessed messages!

### Option 3: Drain Queues Aggressively (Recommended)

1. **Temporarily disable ingester** to stop new messages:
   ```bash
   docker stop backfill-ingester
   ```

2. **Scale up indexers** to drain queue faster

3. **Monitor memory usage** as it decreases:
   ```bash
   watch -n 5 'redis-cli -h localhost -p 6380 INFO memory | grep used_memory_human'
   ```

4. **Re-enable ingester** once memory drops below 80%:
   ```bash
   docker start backfill-ingester
   ```

### Option 4: Emergency Queue Trim (LAST RESORT)
If you need to free memory immediately:

```bash
# Trim streams to keep only recent messages
redis-cli -h localhost -p 6380 XTRIM firehose_live MAXLEN ~ 10000000
redis-cli -h localhost -p 6380 XTRIM firehose_backfill MAXLEN ~ 10000000
```

**WARNING**: This **WILL LOSE DATA**! Only use if:
- Production is completely down
- Other options have failed
- Data loss is acceptable

## Long-Term Solution

### 1. Increase Redis Memory Allocation
Recommend **64GB minimum** for this workload:
```yaml
redis:
  command: redis-server --appendonly yes --appendfsync everysec --maxmemory 64gb --maxmemory-policy noeviction
  volumes:
    - /data/backfill-redis:/data
```

### 2. Implement Stream Trimming
Add automatic trimming to keep streams bounded:

```typescript
// In ingester/indexer
const MAX_STREAM_LENGTH = 50_000_000; // 50M messages max

// After adding messages
await redis.xtrim('firehose_live', 'MAXLEN', '~', MAX_STREAM_LENGTH);
```

### 3. Monitor Memory Continuously
Set up alerts for:
- Redis memory usage > 80%
- Queue depth > 10M messages
- Pending messages > 1M

### 4. Consider Redis Cluster
For high-throughput workloads, consider:
- Redis Cluster for horizontal scaling
- Separate Redis instances for different streams
- KeyDB (Redis alternative with better memory efficiency)

## Monitoring Commands

```bash
# Check memory usage
redis-cli -h localhost -p 6380 INFO memory | grep -E "used_memory_human|maxmemory_human"

# Check queue depths
redis-cli -h localhost -p 6380 XLEN firehose_live
redis-cli -h localhost -p 6380 XLEN firehose_backfill

# Check consumer group lag
redis-cli -h localhost -p 6380 XINFO GROUPS firehose_live

# Watch memory in real-time
watch -n 5 'redis-cli -h localhost -p 6380 INFO memory | grep used_memory_human'
```

## Recommended Action Plan

1. **RIGHT NOW**: Stop ingester to prevent queue growth
2. **Next 1 hour**: Let existing indexers drain queue
3. **Monitor**: Watch memory drop below 80%
4. **Scale up**: Deploy Rust indexers (more efficient) to drain faster
5. **After stabilization**: Re-enable ingester
6. **Next maintenance window**: Increase Redis memory to 64GB
7. **Long-term**: Implement automatic stream trimming

## Why Rust Indexer Helps

The Rust indexer would help once Redis has memory:
- **Lower memory footprint**: 2GB vs 12GB per instance
- **Fewer connections**: 20 vs ~50-100 per instance
- **Faster processing**: Can run at higher concurrency
- **Result**: Drain queues faster with less resource usage

## Status

- [ ] Incident acknowledged
- [ ] Ingester stopped
- [ ] Memory draining
- [ ] Memory below 80%
- [ ] Rust indexers deployed
- [ ] Ingester restarted
- [ ] Redis memory increased to 64GB
- [ ] Monitoring alerts configured
- [ ] Auto-trimming implemented

---

**Discovered while testing rsky-indexer fixes. The indexer code is ready, but production Redis must be fixed first!**
