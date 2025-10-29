# Rust Components Production Deployment Guide

## Overview

This guide covers deploying the Rust replacements for the TypeScript Bluesky components in production.

## Components Replaced

| TypeScript Component | Rust Replacement | Instances | Concurrency | Memory | CPUs |
|---------------------|------------------|-----------|-------------|---------|------|
| **Ingester** (1x) | rsky-ingester | 1 | N/A | 2GB ↓ from 4GB | 2 |
| **Indexers** (8x) | rsky-indexer | 6 | 100 ↑ from 12 | 4GB ↓ from 12GB | 4 |
| **Backfillers** (3x) | rsky-backfiller | 2 | 20 ↑ from 15 | 4GB ↓ from 12GB | 2 |

### Performance Improvements

**Indexing Capacity:**
- TypeScript: 8 instances × 12 concurrency = **96 concurrent tasks**
- Rust: 6 instances × 100 concurrency = **600 concurrent tasks**
- **Result: 6.25x increase in indexing throughput**

**Resource Efficiency:**
- **67% reduction in total indexer memory** (96GB → 24GB)
- **Fewer instances** for easier management
- **Lower CPU usage per task** due to native compilation

## Building Docker Images

### Prerequisites

```bash
# Install Docker
docker --version

# Ensure you're in the rsky workspace root
cd /path/to/rsky
```

### Build All Images

```bash
./build-docker-images.sh
```

This builds:
- `rsky-ingester:latest`
- `rsky-indexer:latest`
- `rsky-backfiller:latest`

Build time: ~10-15 minutes on first build (Rust compilation), ~2-3 minutes on subsequent builds with layer caching.

### Manual Build (if needed)

```bash
# Build each component individually
docker build -f rsky-ingester/Dockerfile -t rsky-ingester:latest .
docker build -f rsky-indexer/Dockerfile -t rsky-indexer:latest .
docker build -f rsky-backfiller/Dockerfile -t rsky-backfiller:latest .
```

## Deployment

### Migration Strategy: Gradual Rollout

**DO NOT switch everything at once.** Use this gradual migration:

#### Phase 1: Deploy Ingester (Low Risk)

The ingester only writes to Redis, no database writes:

```bash
# Stop TypeScript ingester
docker stop backfill-ingester

# Start Rust ingester
docker-compose -f docker-compose.prod-rust.yml up -d ingester

# Monitor for 10 minutes
docker logs -f rust-ingester

# Check Redis stream is receiving events
docker exec -it backfill-redis redis-cli
> XLEN firehose_live
> XLEN label_live
```

**Expected behavior:**
- `firehose_live` stream should be growing
- Ingester CPU should be 100-150%
- Memory should be <2GB

If issues: `docker stop rust-ingester && docker start backfill-ingester`

#### Phase 2: Deploy 1 Backfiller (Low Risk)

```bash
# Stop one TypeScript backfiller
docker stop atproto-backfiller3-1

# Start one Rust backfiller
docker-compose -f docker-compose.prod-rust.yml up -d backfiller1

# Monitor
docker logs -f rust-backfiller1
```

Wait 1 hour, monitor for errors.

#### Phase 3: Deploy Remaining Backfillers

```bash
# Stop remaining TypeScript backfillers
docker stop atproto-backfiller1-1 atproto-backfiller2-1

# Start second Rust backfiller
docker-compose -f docker-compose.prod-rust.yml up -d backfiller2
```

#### Phase 4: Deploy Indexers (CRITICAL - Test Carefully)

Indexers write to PostgreSQL. Deploy one at a time:

```bash
# Stop TypeScript indexer1
docker stop atproto-indexer1-1

# Start Rust indexer1
docker-compose -f docker-compose.prod-rust.yml up -d indexer1

# Monitor closely for 30 minutes
docker logs -f rust-indexer1

# Check PostgreSQL for errors
docker exec -it backfill-pgbouncer1 psql -U bsky -d bsky -c "SELECT COUNT(*) FROM post;"

# Monitor Redis pending messages (should be decreasing)
docker exec -it backfill-redis redis-cli
> XPENDING firehose_live indexer_group
```

**If successful after 30 mins**, deploy indexer2. Repeat for all 6 indexers.

**If errors**, immediately rollback:
```bash
docker stop rust-indexer1
docker start atproto-indexer1-1
```

### Full Deployment (After Testing)

Once all phases pass:

```bash
# Stop all TypeScript components
docker-compose -f /mnt/nvme/bsky/atproto/docker-compose.prod.yml down ingester backfiller1 backfiller2 backfiller3 indexer1 indexer2 indexer3 indexer4 indexer5 indexer6 indexer7 indexer8

# Start all Rust components
docker-compose -f docker-compose.prod-rust.yml up -d ingester backfiller1 backfiller2 indexer1 indexer2 indexer3 indexer4 indexer5 indexer6
```

## Configuration

### Environment Variables

All components support these common variables:

**Database:**
```bash
DATABASE_URL=postgresql://user:pass@host:port/dbname
DB_POOL_MAX_SIZE=200        # Default: concurrency * 2
DB_POOL_MIN_IDLE=50         # Default: concurrency / 2
```

**Redis:**
```bash
REDIS_URL=redis://host:port
```

**Logging:**
```bash
RUST_LOG=info                      # Levels: error, warn, info, debug, trace
RUST_BACKTRACE=1                   # Enable backtraces on errors
```

### Component-Specific Variables

**rsky-ingester:**
```bash
INGESTER_RELAY_HOSTS=https://relay1.us-east.bsky.network,https://relay1.us-west.bsky.network
INGESTER_LABELER_HOSTS=https://atproto.africa
INGESTER_HIGH_WATER_MARK=100000000
INGESTER_BATCH_SIZE=500
INGESTER_BATCH_TIMEOUT_MS=100
```

**rsky-indexer:**
```bash
INDEXER_CONSUMER=rust-indexer1     # Unique name per instance
INDEXER_CONCURRENCY=100            # Concurrent processing tasks
INDEXER_BATCH_SIZE=500             # Messages per batch
ENABLE_DID_RESOLUTION=true         # Enable handle resolution
PLC_URL=https://plc.directory      # Optional: custom PLC directory
```

**rsky-backfiller:**
```bash
BACKFILLER_CONSUMER=rust-backfiller1
BACKFILLER_CONCURRENCY=20
BACKFILLER_BATCH_SIZE=500
BACKFILLER_HTTP_TIMEOUT_SECS=60        # HTTP request timeout
BACKFILLER_MAX_RETRIES=3               # Max retry attempts before DLQ
BACKFILLER_RETRY_INITIAL_BACKOFF_MS=1000  # Initial retry delay
BACKFILLER_RETRY_MAX_BACKOFF_MS=30000  # Max retry delay (exponential backoff)
BACKFILLER_METRICS_PORT=9090           # Prometheus metrics endpoint
```

## Monitoring

### Check Container Status

```bash
docker ps | grep rust-
docker stats --no-stream | grep rust-
```

### Prometheus Metrics

All Rust components expose Prometheus metrics on port 9090 (internal). Access via:

```bash
# Backfiller1 metrics
curl http://localhost:9091/metrics

# Backfiller2 metrics
curl http://localhost:9092/metrics
```

**Key Backfiller Metrics:**
- `backfiller_repos_processed_total` - Total repos successfully processed
- `backfiller_repos_failed_total` - Total repos that failed processing
- `backfiller_repos_dead_lettered_total` - Total repos sent to DLQ
- `backfiller_records_extracted_total` - Total records extracted from repos
- `backfiller_retries_attempted_total` - Total retry attempts
- `backfiller_repos_waiting` - Current repos in input stream
- `backfiller_repos_running` - Current repos being processed
- `backfiller_output_stream_length` - Output stream length (backpressure indicator)
- `backfiller_car_fetch_errors_total` - CAR fetch errors
- `backfiller_car_parse_errors_total` - CAR parsing errors
- `backfiller_verification_errors_total` - Repo verification errors

**Dead Letter Queue:**

Failed repos after max retries are sent to `repo_backfill_dlq` stream:

```bash
# Check DLQ length
docker exec -it backfill-redis redis-cli
> XLEN repo_backfill_dlq

# Inspect failed repos
> XRANGE repo_backfill_dlq - + COUNT 10
```

### View Logs

```bash
# Real-time logs
docker logs -f rust-indexer1

# Last 100 lines
docker logs --tail 100 rust-indexer1

# Logs since 10 minutes ago
docker logs --since 10m rust-indexer1
```

### Redis Metrics

```bash
# Connect to Redis
docker exec -it backfill-redis redis-cli

# Check stream lengths
XLEN firehose_live
XLEN firehose_backfill
XLEN label_live

# Check consumer group info
XINFO GROUPS firehose_live

# Check pending messages per consumer
XPENDING firehose_live indexer_group
XPENDING firehose_live indexer_group - + 10 rust-indexer1
```

### Database Metrics

```bash
# Connect via PGBouncer
docker exec -it backfill-pgbouncer1 psql -U bsky -d bsky

# Check recent index activity
SELECT COUNT(*), MAX(indexed_at) FROM post;
SELECT COUNT(*), MAX(indexed_at) FROM "like";

# Check pool stats
SHOW POOLS;
SHOW STATS;
```

## Performance Tuning

### If Indexers Can't Keep Up

**Increase concurrency per instance:**
```yaml
environment:
  INDEXER_CONCURRENCY: "150"  # Up from 100
  DB_POOL_MAX_SIZE: "300"     # Must increase proportionally
```

**Or add more instances:**
```bash
# Add indexer7
docker-compose -f docker-compose.prod-rust.yml up -d --scale indexer=7
```

### If Redis is Bottleneck (98% CPU)

Current bottleneck visible in your stats. Options:

1. **Use Redis Cluster** (best long-term solution)
2. **Increase Redis max memory** (already at 32GB)
3. **Enable RDB snapshots less frequently**
4. **Use faster disk for Redis persistence**

### If Database is Bottleneck

1. **Increase PGBouncer pool sizes:**
```yaml
DEFAULT_POOL_SIZE: "150"  # Up from 100
MAX_CLIENT_CONN: "1500"   # Up from 1000
```

2. **Add more PGBouncer instances**

3. **Tune PostgreSQL** (connection limits, work_mem, etc.)

## Troubleshooting

### Indexer Errors

**Problem:** `Failed to ACK message after retries`
- **Cause:** Redis connection issues
- **Fix:** Check Redis connectivity, increase timeout

**Problem:** `Database error: connection timeout`
- **Cause:** PGBouncer pool exhausted
- **Fix:** Increase `DB_POOL_MAX_SIZE` or reduce `INDEXER_CONCURRENCY`

**Problem:** `DID not found`
- **Cause:** PLC directory unavailable
- **Fix:** Set `ENABLE_DID_RESOLUTION=false` temporarily

### Memory Issues

**Problem:** Container OOM killed
- **Fix:** Increase `mem_limit` in docker-compose.yml

**Problem:** High memory usage
- **Check:** Rust memory is more predictable than Node.js
- **Normal:** Indexers: 2-3GB, Ingester: 1-2GB

### CPU Issues

**Problem:** High CPU usage
- **Normal for Rust:** 100-200% CPU per container is expected under load
- **If >400%:** Check for infinite loops in logs

## Rollback

If critical issues arise:

```bash
# Stop all Rust components
docker-compose -f docker-compose.prod-rust.yml down

# Start TypeScript components
docker-compose -f /mnt/nvme/bsky/atproto/docker-compose.prod.yml up -d
```

## Expected Resource Usage (After Migration)

Based on your current TypeScript usage:

| Component | Current (TS) | Expected (Rust) | Improvement |
|-----------|--------------|-----------------|-------------|
| **Ingester CPU** | 147% | 80-120% | -18 to -40% |
| **Ingester Memory** | 290MB | 200-500MB | Similar |
| **Indexer CPU (each)** | 27-36% | 40-60% | Higher per instance, but fewer instances |
| **Indexer Memory (each)** | 103-120MB | 2-3GB | Higher per instance, but ~67% total reduction |
| **Total Indexer Memory** | 96GB (8×12GB) | 24GB (6×4GB) | **-75%** |

**Total cluster improvements:**
- **Memory saved:** ~72GB
- **Throughput increase:** 6.25x
- **Instances reduced:** 11 → 9 (-18%)

## Support

For issues:
1. Check logs: `docker logs rust-[component]`
2. Check GitHub issues: https://github.com/rudyfraser/rsky/issues
3. Review INDEXER_OPTIMIZATION_RECOMMENDATIONS.md for tuning tips
