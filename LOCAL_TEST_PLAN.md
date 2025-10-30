# Local Testing Plan for rsky-indexer

## Overview

Test the rsky-indexer against a **real local dataplane** that creates the proper database schema, using your existing test containers.

## Prerequisites

- ✅ Existing test PostgreSQL on port 5433 (rsky-test-postgres)
- ✅ Existing test Redis on port 6379 (rsky-test-redis)
- ✅ atproto repo at `/Users/rudyfraser/Projects/atproto`
- ✅ rsky repo at `/Users/rudyfraser/Projects/rsky`

## Step 1: Build TypeScript Dataplane Image

This will take 5-10 minutes as it builds all TypeScript packages.

### Option A: Build it yourself
```bash
cd /Users/rudyfraser/Projects/rsky

# Build the dataplane image from atproto repo
docker build \
  -f /Users/rudyfraser/Projects/atproto/services/bsky/Dockerfile \
  -t bsky-test:latest \
  /Users/rudyfraser/Projects/atproto

# Verify image was built
docker images | grep bsky-test
```

### Option B: Let me build it
Just say "build the dataplane" and I'll run the docker build command.

## Step 2: Start Dataplane (Creates Database Schema)

```bash
cd /Users/rudyfraser/Projects/rsky

# Start dataplane only (creates schema)
docker compose -f docker-compose.local-test.yml up -d dataplane

# Watch logs - wait for "Migrations complete" or similar message
docker compose -f docker-compose.local-test.yml logs -f dataplane

# You should see messages like:
# - "Running migrations..."
# - "Creating table: actor"
# - "Creating table: record"
# - etc.

# Once migrations complete, press Ctrl+C
```

## Step 3: Verify Database Schema Created

```bash
# Connect to test database
docker exec -it rsky-test-postgres psql -U postgres -d postgres -c "\dt"

# You should see tables like:
# - actor
# - record
# - actor_sync
# - post
# - like
# - follow
# - etc.

# Verify column names (should be quoted camelCase!)
docker exec -it rsky-test-postgres psql -U postgres -d postgres -c "\d record"

# Should show: indexedAt (not indexed_at)
```

## Step 4: Optional - Start API for Testing

```bash
# Start API server
docker compose -f docker-compose.local-test.yml up -d api

# Wait for health check
sleep 10

# Test API
curl http://localhost:3000/xrpc/_health
# Should return: {"version":"..."}
```

## Step 5: Generate Test Events (Two Options)

### Option A: Connect to Real Bluesky Firehose (Recommended)

```bash
# Start ingester - connects to bsky.network for real events
docker compose -f docker-compose.local-test.yml --profile with-ingester up -d ingester

# Watch events flow into Redis
watch -n 2 'docker exec rsky-test-redis redis-cli XLEN firehose_live'

# Should see number increasing!
```

### Option B: Create Manual Test Events

```bash
# I can create a script to insert test messages into Redis
# This gives you full control but requires more setup
```

## Step 6: Run Rust Indexer!

```bash
# Set environment variables
export RUST_LOG="info,rsky_indexer=debug"
export RUST_BACKTRACE="1"
export REDIS_URL="redis://localhost:6379"
export DATABASE_URL="postgresql://postgres:postgres@localhost:5433/postgres"
export INDEXER_STREAMS="firehose_live"
export INDEXER_GROUP="firehose_group"
export INDEXER_CONSUMER="TEST_rust_indexer"
export INDEXER_CONCURRENCY="5"
export INDEXER_BATCH_SIZE="10"
export DB_POOL_MAX_SIZE="20"
export DB_POOL_MIN_IDLE="5"
export INDEXER_MODE="stream"
export ENABLE_DID_RESOLUTION="false"

# Run indexer
cd /Users/rudyfraser/Projects/rsky
./target/release/indexer

# You should see:
# INFO Starting rsky-indexer
# INFO PostgreSQL pool configured: max_size=20, concurrency=5
# INFO Connected to PostgreSQL
# INFO Starting stream indexers for 1 streams
# INFO Starting StreamIndexer for stream: ["firehose_live"]
# INFO Processed batch of 10 messages (or similar)
```

## Step 7: Verify Processing

```bash
# Check that records are being written
docker exec -it rsky-test-postgres psql -U postgres -d postgres -c \
  "SELECT COUNT(*) FROM record;"

# Should see numbers increasing!

# Check specific record
docker exec -it rsky-test-postgres psql -U postgres -d postgres -c \
  "SELECT uri, \"indexedAt\" FROM record LIMIT 5;"

# Check Redis queue decreasing
docker exec rsky-test-redis redis-cli XLEN firehose_live
```

## Step 8: Monitor & Validate

```bash
# Watch indexer logs
# (indexer should be running in terminal)

# In another terminal, watch database growth
watch -n 5 'docker exec -it rsky-test-postgres psql -U postgres -d postgres -c "SELECT COUNT(*) FROM record;"'

# Watch queue depth
watch -n 2 'docker exec rsky-test-redis redis-cli XLEN firehose_live'

# Check for errors in indexer logs
# Should see NO "column does not exist" errors!
# Should see NO "connection pool" errors!
# Should see NO panics!
```

## Expected Results

### ✅ Success Indicators
- Dataplane starts and runs migrations
- Database tables created with correct schema
- Ingester connects and adds messages to Redis
- Rust indexer starts without errors
- Messages processed successfully
- Records appear in database
- Queue depth decreases
- No panics or crashes

### ❌ Failure Indicators
- "column does not exist" errors → Schema mismatch
- "connection refused" errors → Connection issues
- Panics → Unwrap errors or other bugs
- "max connections" errors → Pool exhaustion
- No records in database → Processing failed

## Cleanup

```bash
# Stop all test services
cd /Users/rudyfraser/Projects/rsky
docker compose -f docker-compose.local-test.yml down

# Optionally clear test data
docker exec rsky-test-redis redis-cli FLUSHALL
docker exec -it rsky-test-postgres psql -U postgres -d postgres -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
```

## Troubleshooting

### Dataplane won't build
```bash
# Check atproto repo status
cd /Users/rudyfraser/Projects/atproto
git status
git checkout divy/backfill

# Ensure pnpm is available in the build
# (Dockerfile uses corepack to enable pnpm)
```

### Dataplane won't start
```bash
# Check logs
docker compose -f docker-compose.local-test.yml logs dataplane

# Common issues:
# - Can't connect to database: Check port 5433 is correct
# - Can't connect to Redis: Check port 6379 is correct
# - Permission errors: Check file permissions
```

### Rust indexer errors
```bash
# Check all fixes are applied
git diff rsky-indexer/src/indexing/mod.rs

# Rebuild
cargo build --release

# Check environment variables
env | grep -E "REDIS|DATABASE|INDEXER"
```

## Files Created

- `docker-compose.local-test.yml` - Test docker-compose configuration
- `LOCAL_TEST_PLAN.md` - This file
- `test-indexer.sh` - Convenience script (optional)

## Next Steps

After successful local testing:
1. Document any issues found
2. Fix any bugs discovered
3. Update deployment documentation
4. Deploy to production (after Redis OOM is fixed!)

---

**Ready to test!** Choose Step 1 Option A or B to begin.
