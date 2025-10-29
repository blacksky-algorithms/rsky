# Integration Tests

This directory contains integration tests for the rsky data pipeline.

## Overview

The integration test (`integration_test.rs`) verifies the complete data flow through the pipeline:

```
Bluesky Network (bsky.network)
  ↓
Ingester (rsky-ingester)
  ↓ writes to
Redis Streams (firehose_live, repo_backfill)
  ↓
Backfiller (rsky-backfiller) ← fetches full repos
  ↓ writes to
Redis Streams (firehose_backfill)
  ↓
Indexer (rsky-indexer)
  ↓ writes to
PostgreSQL
```

## Prerequisites

Before running the integration tests, you need:

### 1. Redis

Start Redis locally:

```bash
# Using Docker
docker run -d -p 6379:6379 redis:latest

# Or using redis-server
redis-server
```

### 2. PostgreSQL

Start PostgreSQL with a test database:

```bash
# Using Docker
docker run -d \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=bsky_test \
  postgres:15

# Or create database manually
createdb bsky_test
```

### 3. Network Access

The test connects to `bsky.network` to fetch real firehose events, so you need:
- Internet connection
- No firewall blocking outbound HTTPS

## Running the Tests

### Basic Run

```bash
# Run the integration test (it's marked as #[ignore] by default)
cargo test --test integration_test -- --ignored --nocapture
```

### With Custom Infrastructure

Use environment variables to point to custom Redis/PostgreSQL:

```bash
export TEST_REDIS_URL="redis://localhost:6379"
export TEST_DATABASE_URL="postgres://postgres:postgres@localhost:5432/bsky_test"

cargo test --test integration_test -- --ignored --nocapture
```

### With Verbose Logging

```bash
RUST_LOG=debug cargo test --test integration_test -- --ignored --nocapture
```

## What the Test Does

### Step 1: Start Ingester
- Connects to `bsky.network` firehose
- Ingests events into Redis `firehose_live` stream
- Runs for 10 seconds

### Step 2: Verify Live Stream
- Checks that `firehose_live` has events
- Logs the count

### Step 3: Queue Backfill
- Adds a test repo (`did:plc:z72i7hdynmk6r22z27h6tvur` - bsky.app) to `repo_backfill` stream

### Step 4: Start Backfiller
- Fetches the queued repo from `bsky.network`
- Parses the CAR file
- Verifies signatures
- Writes records to `firehose_backfill` stream
- Waits up to 60 seconds for completion

### Step 5: Start Indexer
- Reads from both `firehose_live` and `firehose_backfill` streams
- Indexes records into PostgreSQL
- Runs for 10 seconds

### Step 6: Verify PostgreSQL
- Counts records in tables:
  - `record` (generic record table)
  - `post` (post-specific table)
  - `actor_sync` (commit tracking)
- Asserts that data was indexed

### Step 7: Sample Records
- Fetches 5 sample records
- Logs their details
- Verifies they exist

### Cleanup
- Stops all services
- Drops test tables
- Clears Redis streams

## Expected Output

Successful test output should look like:

```
Running 1 test
test test_full_pipeline ...
INFO Starting integration test
INFO Step 1: Starting ingester
INFO Step 2: Verifying firehose_live stream
INFO Found 42 events in firehose_live
INFO Step 3: Queueing repo for backfill
INFO Step 4: Starting backfiller
INFO Found 156 backfill events
INFO Step 5: Starting indexer
INFO Step 6: Verifying PostgreSQL data
INFO PostgreSQL stats: {"record": 198, "post": 23, "actor_sync": 1}
INFO Step 7: Verifying specific records
INFO Sample record: uri=at://did:plc:.../app.bsky.feed.post/..., collection=app.bsky.feed.post
INFO Test complete, cleaning up
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured
```

## Troubleshooting

### Test Hangs or Times Out

**Problem**: Ingester can't connect to bsky.network
- Check internet connection
- Verify `bsky.network` is accessible: `curl https://bsky.network/xrpc/com.atproto.sync.subscribeRepos`

**Problem**: Backfiller times out
- The test repo might be large or slow to fetch
- Network issues
- Try increasing timeout in test code

**Problem**: No data in PostgreSQL
- Check indexer logs for errors
- Verify Redis streams have data: `redis-cli XLEN firehose_live`
- Check PostgreSQL connection

### Connection Errors

**Redis connection failed**:
```bash
# Verify Redis is running
redis-cli ping
# Should return "PONG"
```

**PostgreSQL connection failed**:
```bash
# Verify PostgreSQL is running
psql -U postgres -d bsky_test -c "SELECT 1;"
# Should return 1
```

### Permission Issues

If you get permission errors on PostgreSQL:
```bash
# Grant permissions
psql -U postgres -d bsky_test -c "GRANT ALL ON DATABASE bsky_test TO postgres;"
```

## Test Infrastructure with Docker Compose

For convenience, here's a `docker-compose.yml` for test infrastructure:

```yaml
version: '3.8'

services:
  redis:
    image: redis:latest
    ports:
      - "6379:6379"

  postgres:
    image: postgres:15
    ports:
      - "5432:5432"
    environment:
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: bsky_test
    volumes:
      - postgres_data:/var/lib/postgresql/data

volumes:
  postgres_data:
```

Start infrastructure:
```bash
docker-compose up -d
```

Run tests:
```bash
cargo test --test integration_test -- --ignored --nocapture
```

Stop infrastructure:
```bash
docker-compose down -v
```

## CI/CD

For CI/CD environments, use the Docker Compose approach:

```yaml
# .github/workflows/integration-test.yml
name: Integration Tests

on: [push, pull_request]

jobs:
  integration:
    runs-on: ubuntu-latest

    services:
      redis:
        image: redis:latest
        ports:
          - 6379:6379

      postgres:
        image: postgres:15
        env:
          POSTGRES_PASSWORD: postgres
          POSTGRES_DB: bsky_test
        ports:
          - 5432:5432

    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
      - name: Run integration tests
        run: cargo test --test integration_test -- --ignored --nocapture
```

## Notes

- Test is marked `#[ignore]` because it requires external infrastructure
- Test connects to real `bsky.network`, so it's not fully hermetic
- For fully isolated tests, consider mocking the firehose endpoint
- Test creates and drops tables, so use a dedicated test database
