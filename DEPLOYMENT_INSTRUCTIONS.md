# rsky-ingester Production Deployment Instructions

## Current Status

✅ **Instrumented build ready** - Exit tracing added to identify restart cause
✅ **Local testing complete** - Verified exit traces work correctly
⏳ **Production deployment pending** - Ready to deploy and capture logs

## What We Built

### Exit Tracing System

Added comprehensive instrumentation to `rsky-ingester/src/bin/ingester.rs` that:

1. **Logs every code path** with unique identifiers (EXIT PATH A-Z)
2. **Uses dual logging** (`error!` + `eprintln!`) to bypass buffer issues
3. **Captures exact exit point** when any ingester task completes
4. **Shows Result values** to distinguish Ok vs Err exits

### Local Test Results

When testing with SIGKILL (external termination):
- ✅ EXIT-TRACE messages appear during startup
- ✅ All three tasks (firehose, backfill, labeler) start successfully
- ✅ When killed externally: **NO EXIT-TRACE** after "Entering tokio::select!"
- ✅ Exit code 137 (128 + 9 = SIGKILL)

**Key Insight**: External kills (SIGKILL, OOM) produce NO exit trace logs after initialization.

## Deployment Steps

### 1. On Your Local Machine

Push the instrumented code to your branch:

```bash
cd /Users/rudyfraser/Projects/rsky
git status  # Verify changes to rsky-ingester/src/bin/ingester.rs
git add rsky-ingester/src/bin/ingester.rs
git add RESTART_LOOP_INVESTIGATION.md
git add DEPLOYMENT_INSTRUCTIONS.md
git commit -m "Add exit tracing instrumentation to debug restart loop"
git push origin rude1/backfill  # Or your branch name
```

### 2. On Production Server

```bash
# Navigate to atproto directory
cd /mnt/nvme/bsky/atproto

# Pull latest changes
git pull origin rude1/backfill  # Use your branch name

# Build instrumented ingester
docker build -f ../rsky/rsky-ingester/Dockerfile -t rsky-ingester:debug ../rsky

# Stop current ingester
docker compose -f docker-compose.prod-rust.yml stop ingester
docker compose -f docker-compose.prod-rust.yml rm -f ingester

# Update docker-compose.prod-rust.yml to use debug image
# Change: image: rsky-ingester:latest
# To:     image: rsky-ingester:debug

# Start with full log capture
docker compose -f docker-compose.prod-rust.yml up ingester 2>&1 | tee /tmp/ingester_debug.log
```

This will:
- Display logs in real-time on console
- Save complete logs to `/tmp/ingester_debug.log`
- Capture ALL output including `eprintln!` messages
- Show restart sequences as they happen

### 3. Capture at Least One Restart Cycle

Let it run until you see at least one complete restart cycle (~30 seconds based on previous behavior). Then stop with Ctrl+C.

### 4. Analyze the Logs

```bash
# Extract EXIT-TRACE messages
grep 'EXIT-TRACE' /tmp/ingester_debug.log

# Expected output - one of these patterns:
```

#### Pattern 1: Task Exits (Application Bug)
```
[EXIT-TRACE] Running in 'all' mode
[EXIT-TRACE] Spawning firehose and backfill tasks
[EXIT-TRACE] Spawning labeler task (hosts configured)
[EXIT-TRACE] Entering tokio::select! with all three tasks
[EXIT-TRACE] tokio::select! triggered on labeler_handle - EXIT PATH F
[EXIT-TRACE] Labeler result: Ok(Err(IngesterError::...))
```
**Diagnosis**: Labeler task exited with an error → **Bug in labeler.rs**

#### Pattern 2: External Kill (OOM/SIGKILL)
```
[EXIT-TRACE] Running in 'all' mode
[EXIT-TRACE] Spawning firehose and backfill tasks
[EXIT-TRACE] Spawning labeler task (hosts configured)
[EXIT-TRACE] Entering tokio::select! with all three tasks
<container restarts - NO MORE EXIT-TRACE>
```
**Diagnosis**: Process killed externally → **Check OOM logs**

### 5. Check for OOM Kill

If Pattern 2 (no exit traces after startup):

```bash
# Check system logs for OOM kills
dmesg | grep -i 'killed process' | grep -i ingester

# Check Docker memory stats
docker stats rust-ingester --no-stream

# Check configured memory limit
docker inspect rust-ingester | jq '.[0].HostConfig.Memory'
# Current limit: 6442450944 (6GB)

# If OOM killed, you'll see in dmesg:
# Out of memory: Killed process <PID> (ingester) ...
```

## Resolution Paths

### If OOM Kill (Pattern 2)

The container is hitting the 6GB memory limit. Solutions:

1. **Increase memory limit** (quick fix):
   ```yaml
   # In docker-compose.prod-rust.yml
   mem_limit: 8g  # Increase from 6g
   ```

2. **Reduce memory usage** (proper fix):
   ```yaml
   # Reduce backpressure threshold
   INGESTER_HIGH_WATER_MARK: "500000"  # From 1000000

   # Reduce batch sizes
   INGESTER_BATCH_SIZE: "250"  # From 500
   ```

3. **Monitor and adjust**:
   ```bash
   # Watch memory usage in real-time
   watch -n 1 'docker stats rust-ingester --no-stream'
   ```

### If Task Exit (Pattern 1)

The EXIT PATH identifier shows which component exited:
- **EXIT PATH D**: Firehose task → check `rsky-ingester/src/firehose.rs`
- **EXIT PATH E**: Backfill task → check `rsky-ingester/src/backfill.rs`
- **EXIT PATH F**: Labeler task → check `rsky-ingester/src/labeler.rs`

The Result value shows what error occurred:
- `Ok(Ok(()))` → Logic bug, infinite loop exited (should never happen!)
- `Ok(Err(IngesterError::...))` → Error was returned, check error type
- `Err(JoinError)` → Task panicked (panic hook will show details)

## Expected Timeline

1. **Deploy instrumented build**: 10 minutes
2. **Wait for restart**: Up to 30 seconds
3. **Analyze logs**: 5 minutes
4. **Implement fix**: 15-60 minutes (depending on root cause)
5. **Re-deploy and verify**: 10 minutes

Total: **~1-2 hours** from start to stable deployment

## Success Criteria

After deploying the fix:
- ✅ No container restarts for 1+ hour
- ✅ All three ingester types running continuously
- ✅ No EXIT-TRACE messages after initial startup
- ✅ Memory usage stable (check with `docker stats`)
- ✅ Redis streams being processed (check lengths)

## Rollback Plan

If issues occur during deployment:

```bash
# Stop the debug ingester
docker compose -f docker-compose.prod-rust.yml stop ingester

# Revert to previous image
# In docker-compose.prod-rust.yml:
#   image: rsky-ingester:latest

# Restart with original image
docker compose -f docker-compose.prod-rust.yml up -d ingester
```

## Reference Files

- **Instrumented code**: `rsky-ingester/src/bin/ingester.rs`
- **Investigation guide**: `RESTART_LOOP_INVESTIGATION.md`
- **Previous analysis**: `INGESTER_CRASH_INVESTIGATION.md`
- **This document**: `DEPLOYMENT_INSTRUCTIONS.md`

## Questions & Support

If you encounter unexpected behavior:

1. **Share the full EXIT-TRACE output**:
   ```bash
   grep 'EXIT-TRACE' /tmp/ingester_debug.log
   ```

2. **Share CRITICAL errors if any**:
   ```bash
   grep 'CRITICAL' /tmp/ingester_debug.log
   ```

3. **Share panic messages if any**:
   ```bash
   grep -A 20 'FATAL PANIC' /tmp/ingester_debug.log
   ```

4. **Share OOM status**:
   ```bash
   dmesg | grep -i 'killed process' | tail -20
   ```

With this information, we can quickly identify the root cause and implement the fix.

---

**Next Step**: Deploy to production and capture the logs from one restart cycle.
