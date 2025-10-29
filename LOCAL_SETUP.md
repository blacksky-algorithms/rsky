# Local Development Setup

This guide explains how to run the Rust ingester/indexer/backfiller locally for testing.

## Prerequisites

- Docker and Docker Compose installed
- At least 4GB RAM available for Docker
- Rust toolchain (if building locally instead of using Docker)

## Architecture Comparison

### Production (docker-compose.prod-rust.yml)
- Redis: 32GB max memory
- 2 PGBouncers (connection pooling)
- 2 Dataplane instances (15GB each)
- 1 API server (8GB)
- 1 Rust ingester (2GB, 3 relays)
- 2 Rust backfillers (4GB each, concurrency=20)
- 6 Rust indexers (4GB each, concurrency=100)
- Prometheus monitoring

### Local (docker-compose.local.yml)
- Redis: 2GB max memory
- Postgres: Direct connection (no PGBouncer)
- 1 Rust ingester (1GB, 1 relay, debug logging)
- 1 Rust backfiller (1GB, concurrency=5) - optional with --profile full
- 1 Rust indexer (1GB, concurrency=10) - optional with --profile full
- No Dataplane/API/Prometheus (not needed for testing)

## Quick Start

### 1. Build Docker Images

First, build all three Rust services:

```bash
# From the rsky repository root
docker build --no-cache -t rsky-ingester:latest -f rsky-ingester/Dockerfile .
docker build --no-cache -t rsky-backfiller:latest -f rsky-backfiller/Dockerfile .
docker build --no-cache -t rsky-indexer:latest -f rsky-indexer/Dockerfile .
```

### 2. Start Minimal Setup (Ingester + Redis only)

This is the fastest way to test just the ingester:

```bash
docker compose -f docker-compose.local.yml up -d redis ingester
```

Watch logs:
```bash
docker compose -f docker-compose.local.yml logs -f ingester
```

### 3. Start Full Pipeline (Ingester + Backfiller + Indexer)

This requires the database schema to be initialized:

```bash
# Start all services including postgres
docker compose -f docker-compose.local.yml --profile full up -d

# Wait for postgres to be ready
docker compose -f docker-compose.local.yml exec postgres pg_isready -U bsky

# Run database migrations (you'll need to create this script)
# docker compose -f docker-compose.local.yml exec postgres psql -U bsky -d bsky -f /path/to/schema.sql
```

## Monitoring

### Check Redis Streams

```bash
# Connect to local Redis
docker exec -it rsky-redis-local redis-cli

# Check stream lengths
XLEN firehose_live
XLEN repo_backfill
XLEN label_live

# Check cursors
KEYS *cursor*
GET firehose_live:cursor:bsky.network

# Monitor stream in real-time
XREAD COUNT 10 STREAMS firehose_live 0
```

### Check Logs

```bash
# All services
docker compose -f docker-compose.local.yml logs -f

# Just ingester
docker compose -f docker-compose.local.yml logs -f ingester

# Just indexer
docker compose -f docker-compose.local.yml --profile full logs -f indexer
```

### Check Container Status

```bash
docker compose -f docker-compose.local.yml ps
```

## Debugging

### Clear Redis Data

If you want to start fresh:

```bash
docker exec -it rsky-redis-local redis-cli FLUSHALL
```

### Restart Ingester

```bash
docker compose -f docker-compose.local.yml restart ingester
```

### Rebuild After Code Changes

```bash
# Rebuild ingester
docker build --no-cache -t rsky-ingester:latest -f rsky-ingester/Dockerfile .

# Restart the container
docker compose -f docker-compose.local.yml restart ingester
```

## Configuration

All configuration is in `docker-compose.local.yml`. Key differences from prod:

### Ingester
- `INGESTER_RELAY_HOSTS`: `bsky.network` (1 relay vs 3 in prod)
- `INGESTER_HIGH_WATER_MARK`: `10000` (vs 100M in prod)
- `INGESTER_BATCH_SIZE`: `100` (vs 500 in prod)
- `RUST_LOG`: `debug` for verbose logging

### Backfiller (optional)
- `BACKFILLER_CONCURRENCY`: `5` (vs 20 in prod)
- `BACKFILLER_BATCH_SIZE`: `100` (vs 500 in prod)

### Indexer (optional)
- `INDEXER_CONCURRENCY`: `10` (vs 100 in prod)
- `DB_POOL_MAX_SIZE`: `20` (vs 200 in prod)

## Expected Behavior

### Successful Ingester Startup

You should see logs like:
```
INFO Starting rsky-ingester
INFO Configuration: IngesterConfig { ... }
INFO Starting FirehoseIngester for bsky.network
INFO Connecting to wss://bsky.network/xrpc/com.atproto.sync.subscribeRepos with cursor 0
```

### Events Being Processed

You should see debug logs of events being received and batched:
```
DEBUG Received #commit message
DEBUG Batching event: Create { ... }
DEBUG Writing batch of 100 events to Redis
```

### Common Issues

**"Connection refused to redis:6379"**
- Redis isn't running yet. Wait for healthcheck to pass.

**"cursor=1 in logs"**
- This was a bug in production. Should now show `cursor=0` for new subscriptions.

**"No events being processed"**
- Check Redis: `docker exec -it rsky-redis-local redis-cli XLEN firehose_live`
- Should be increasing over time
- If not, check ingester logs for errors

**"CBOR decoding errors"**
- These are now handled gracefully and should not crash the ingester
- Check logs - should show "Failed to decode" warnings but continue processing

## Stopping Services

```bash
# Stop all
docker compose -f docker-compose.local.yml down

# Stop and remove volumes (clears all data)
docker compose -f docker-compose.local.yml down -v
```

## Next Steps

1. **Test ingester locally** - Verify events flow into Redis
2. **Check for cursor=1 bug** - Should show cursor=0 initially
3. **Monitor for crashes** - Ingester should stay running
4. **Verify event processing** - XLEN should increase on firehose_live stream
5. **Test full pipeline** - Add indexer/backfiller and test end-to-end

## Troubleshooting Production Issues

The main issues we saw in production:

1. ✅ **Fixed:** CBOR decoding errors - Now handled gracefully
2. ✅ **Fixed:** Unknown #sync messages - Now filtered correctly
3. ❓ **Testing:** cursor=1 appearing instead of cursor=0
4. ❓ **Testing:** No firehose events being processed

Use local testing to verify these issues are resolved before deploying to prod.
