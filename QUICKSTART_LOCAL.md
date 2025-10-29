# Quick Start - Local Testing

This is a simplified guide to get you testing the Rust ingester locally in 5 minutes.

## What You'll Get Running

1. **Redis** - Local Redis instance (2GB max memory)
2. **Rust Ingester** - Connects to `bsky.network` and streams events to Redis

Optional (with `--profile full`):
3. **Postgres** - Local database
4. **Rust Backfiller** - Fetches repository history
5. **Rust Indexer** - Processes events and writes to database

## Step-by-Step

### 1. Build the Ingester Image

```bash
cd /Users/rudyfraser/Projects/rsky
./local-dev.sh build-ingester
```

This will build the Rust ingester Docker image using the nightly Rust toolchain.

### 2. Start Redis + Ingester

```bash
./local-dev.sh start
```

This starts:
- `rsky-redis-local` container on port 6379
- `rsky-ingester-local` container

### 3. Watch the Logs

```bash
./local-dev.sh logs-ingester
```

**What to look for:**

✅ **Good signs:**
```
INFO Starting rsky-ingester
INFO Starting FirehoseIngester for bsky.network
INFO Connecting to wss://bsky.network/xrpc/com.atproto.sync.subscribeRepos with cursor 0
DEBUG Received #commit message
DEBUG Batching event
```

❌ **Bad signs:**
```
cursor=1          (should be cursor=0)
Connection refused
WebSocket error
Container keeps restarting
```

### 4. Check Redis Stream

In another terminal:

```bash
./local-dev.sh redis-stats
```

You should see:
```
Firehose Live Stream:
(integer) 1523   <- This number should be growing!

Repo Backfill Stream:
(integer) 0

Label Live Stream:
(integer) 0

Cursors:
1) "firehose_live:cursor:bsky.network"
```

Check the cursor value:

```bash
./local-dev.sh redis
> GET firehose_live:cursor:bsky.network
```

Should show a large number (like `37234567`), NOT `1`!

### 5. Verify Events Are Flowing

```bash
./local-dev.sh redis
> XLEN firehose_live
(integer) 5423

# Wait 10 seconds and check again
> XLEN firehose_live
(integer) 5892   <- Should be higher!

# Look at an event
> XREAD COUNT 1 STREAMS firehose_live 0
```

## Helper Script Commands

```bash
./local-dev.sh build           # Build all images
./local-dev.sh build-ingester  # Build only ingester

./local-dev.sh start           # Start minimal (ingester + redis)
./local-dev.sh start-full      # Start full pipeline

./local-dev.sh logs-ingester   # Watch ingester logs
./local-dev.sh redis-stats     # Check Redis streams
./local-dev.sh redis           # Open Redis CLI
./local-dev.sh redis-clear     # Clear all Redis data

./local-dev.sh status          # Show container status
./local-dev.sh stop            # Stop everything
./local-dev.sh clean           # Stop and remove all data
```

## Testing Checklist

Once running locally, verify these fixes from production:

- [ ] Ingester connects successfully to bsky.network
- [ ] Initial cursor is `0` (not `1`)
- [ ] Events are being written to Redis (XLEN increases)
- [ ] No CBOR decoding errors crash the ingester
- [ ] #sync messages are handled gracefully (logged but not errors)
- [ ] Cursor updates to large numbers (sequence numbers)
- [ ] Container stays running (doesn't crash)
- [ ] Debug logs show events being processed

## Debugging Issues

### Container won't start
```bash
# Check status
./local-dev.sh status

# See what failed
docker compose -f docker-compose.local.yml logs
```

### Still seeing cursor=1
```bash
# Clear Redis and restart
./local-dev.sh redis-clear
docker compose -f docker-compose.local.yml restart ingester
./local-dev.sh logs-ingester
```

### No events in Redis
```bash
# Check if ingester is running
./local-dev.sh status

# Check logs for errors
./local-dev.sh logs-ingester

# Check network connectivity
docker exec rsky-ingester-local ping -c 3 bsky.network
```

### Want to rebuild after code changes
```bash
# Make your code changes, then:
./local-dev.sh build-ingester
docker compose -f docker-compose.local.yml restart ingester
./local-dev.sh logs-ingester
```

## Next Steps

Once the ingester is working locally:

1. **Test for 10-15 minutes** - Verify no crashes, events keep flowing
2. **Compare logs to production** - Make sure we fixed the issues
3. **Check for the cursor=1 bug** - This was the mystery in prod
4. **Verify CBOR error handling** - Should log warnings but not crash

If everything looks good, we can:
- Deploy the fixed images to production
- Or add the indexer/backfiller to local setup for full pipeline testing

## File Structure

```
/Users/rudyfraser/Projects/rsky/
├── docker-compose.local.yml    # Local docker-compose config
├── local-dev.sh                # Helper script (this makes life easier!)
├── LOCAL_SETUP.md              # Detailed documentation
└── QUICKSTART_LOCAL.md         # This file
```

## Differences from Production

| Component | Production | Local |
|-----------|-----------|-------|
| Redis Memory | 32GB | 2GB |
| Relay Hosts | 3 relays | 1 relay (bsky.network) |
| Batch Size | 500 | 100 |
| High Water Mark | 100M | 10K |
| Logging | INFO | DEBUG (verbose) |
| Backfiller Concurrency | 20 | 5 |
| Indexer Concurrency | 100 | 10 |
| Database | PGBouncer → Postgres | Direct Postgres |
| Memory Limits | 2-4GB per service | 1GB per service |

The local setup is designed to be resource-friendly while still testing the core functionality.
