# rsky-ingester Restart Loop Investigation

## Current Status

The ingester container is restarting every ~20-30 seconds in production. The process exits cleanly (no panic messages), triggering Docker's `restart: unless-stopped` policy.

## Key Mystery

**The application code has comprehensive error handling that should log "CRITICAL" messages before exit, but these messages are NOT appearing in the partial logs captured so far.**

## Investigation Approach

### Phase 1: Instrumented Build (COMPLETED)

I've added comprehensive exit tracing to `rsky-ingester/src/bin/ingester.rs`:

1. **Exit Path Identifiers**: Each possible exit point now logs a unique identifier (EXIT PATH A-Z)
2. **Dual Logging**: Uses both `error!()` (tracing) AND `eprintln!()` (direct stderr)
   - This bypasses potential logging buffer issues
   - `eprintln!` writes immediately to stderr, visible in docker logs
3. **Breadcrumbs**: Logs before entering critical sections (tokio::select!, task spawning)
4. **Canary Code**: Added unreachable code at end of main() to detect impossible code paths

### What the Exit Traces Will Show

When you see these messages in logs, they indicate which code path triggered the exit:

- `[EXIT-TRACE] Running in 'all' mode` - Confirmed running all three ingester types
- `[EXIT-TRACE] Spawning labeler task (hosts configured)` - Labeler task was spawned
- `[EXIT-TRACE] Entering tokio::select! with all three tasks` - Waiting for any task to exit
- `[EXIT-TRACE] tokio::select! triggered on X - EXIT PATH Y` - Which task exited first
- `[EXIT-TRACE] X result: ...` - The Result value from the exited task

**EXIT PATH IDENTIFIERS**:
- **A**: Firehose-only mode exited
- **B**: Backfill-only mode exited
- **C**: Labeler-only mode exited
- **D**: Firehose task exited (in 'all' mode with labeler)
- **E**: Backfill task exited (in 'all' mode with labeler)
- **F**: Labeler task exited (in 'all' mode)
- **G**: Firehose task exited (in 'all' mode, no labeler)
- **H**: Backfill task exited (in 'all' mode, no labeler)
- **I**: Unknown INGESTER_MODE
- **Z**: IMPOSSIBLE - reached end of main() (should never happen)

### Phase 2: Deploy and Capture Full Logs (NEXT STEP)

On the production server:

```bash
# 1. Build the instrumented image
cd /mnt/nvme/bsky/atproto
git pull origin rude1/backfill  # Or your branch name
docker build -f ../rsky/rsky-ingester/Dockerfile -t rsky-ingester:debug ../rsky

# 2. Update docker-compose to use debug image
# Edit docker-compose.prod-rust.yml:
#   image: rsky-ingester:debug

# 3. Stop current ingester
docker compose -f docker-compose.prod-rust.yml stop ingester
docker compose -f docker-compose.prod-rust.yml rm -f ingester

# 4. Start with log capture
docker compose -f docker-compose.prod-rust.yml up ingester 2>&1 | tee /tmp/ingester_debug.log

# This will:
# - Stream logs to console AND file
# - Capture ALL output including eprintln! messages
# - Show the restart sequence in real-time
```

### Phase 3: Analyze Captured Logs

Look for the `[EXIT-TRACE]` messages in the order they appear:

#### Scenario 1: Task Exits Immediately
```
[EXIT-TRACE] Running in 'all' mode
[EXIT-TRACE] Spawning firehose and backfill tasks
[EXIT-TRACE] Spawning labeler task (hosts configured)
[EXIT-TRACE] Entering tokio::select! with all three tasks
[EXIT-TRACE] tokio::select! triggered on labeler_handle - EXIT PATH F
[EXIT-TRACE] Labeler result: Ok(Err(...))
```
**Diagnosis**: Labeler task's run_labeler() function returned an error

#### Scenario 2: Task Panics
```
[EXIT-TRACE] Running in 'all' mode
[EXIT-TRACE] Spawning firehose and backfill tasks
[EXIT-TRACE] Spawning labeler task (hosts configured)
[EXIT-TRACE] Entering tokio::select! with all three tasks
[FATAL PANIC OCCURRED]
Location: rsky-ingester/src/labeler.rs:123:45
```
**Diagnosis**: Panic in labeler.rs (panic hook will show full backtrace)

#### Scenario 3: Run Function Completes Successfully (Bug!)
```
[EXIT-TRACE] Running in 'all' mode
...
[EXIT-TRACE] tokio::select! triggered on firehose_handle - EXIT PATH D
[EXIT-TRACE] Firehose result: Ok(Ok(()))
```
**Diagnosis**: run_firehose() returned Ok(()) despite infinite loop - LOGIC BUG

#### Scenario 4: External Termination (SIGKILL/OOM) - **CONFIRMED VIA LOCAL TEST**
```
[EXIT-TRACE] Running in 'all' mode
[EXIT-TRACE] Spawning firehose and backfill tasks
[EXIT-TRACE] Spawning labeler task (hosts configured)
[EXIT-TRACE] Entering tokio::select! with all three tasks
<container restarts with no further EXIT-TRACE messages>
```
**Diagnosis**: Process killed externally by SIGKILL or OOM killer

**Evidence**: Local testing shows SIGKILL produces exit code 137 and NO exit traces after "Entering tokio::select!"

**Next Steps**:
```bash
# Check for OOM kills in system logs
dmesg | grep -i 'killed process' | grep ingester

# Check Docker memory usage before restart
docker stats rust-ingester --no-stream

# Check if container hit memory limit
docker inspect rust-ingester | jq '.[0].HostConfig.Memory'
```

## Expected Behavior vs Reality

### Expected (Correct Operation)
The ingester should run forever. None of the EXIT-TRACE messages should appear after startup completes.

### Bug Patterns to Recognize

1. **Infinite Loop Exit Bug**
   - `result: Ok(Ok(()))` means the run() function returned despite having `loop { ... }`
   - Check for early returns in the loop
   - Check for break statements
   - Check if WebSocket connection closes without retry

2. **Panic in Spawned Task**
   - `result: Err(JoinError)` means the task panicked
   - Panic hook will show location and backtrace
   - Look for `.unwrap()`, `.expect()`, or array indexing

3. **Error Propagation**
   - `result: Ok(Err(IngesterError::...))` means function returned error
   - Check why error wasn't caught and retried in the infinite loop
   - Look at the error type to identify source (Redis, WebSocket, etc.)

4. **External Kill**
   - No EXIT-TRACE after "Entering tokio::select!"
   - Check `dmesg` for OOM kills
   - Check Docker health checks (none configured currently)
   - Check for manual kills or automation

## Deployment Checklist

Before deploying to production:

- [ ] Built with instrumentation: `cargo build --release --bin ingester`
- [ ] Docker image created: `docker build -f rsky-ingester/Dockerfile -t rsky-ingester:debug .`
- [ ] Log capture ready: `tee /tmp/ingester_debug.log`
- [ ] Access to real-time logs: `docker logs -f rust-ingester`
- [ ] Access to container inspection: `docker inspect rust-ingester`
- [ ] Access to system logs: `dmesg | tail -100`

## After Capturing Logs

### Extract Key Information

```bash
# Find all EXIT-TRACE messages
grep 'EXIT-TRACE' /tmp/ingester_debug.log

# Find CRITICAL errors
grep 'CRITICAL' /tmp/ingester_debug.log

# Find panics
grep -A 20 'FATAL PANIC' /tmp/ingester_debug.log

# Get timeline of restarts
grep 'Starting rsky-ingester' /tmp/ingester_debug.log | nl

# Check for OOM kills
dmesg | grep -i 'killed process'

# Check exit codes
docker inspect rust-ingester | jq '.[0].State'
```

### Timing Analysis

```bash
# Extract timestamps of key events
grep -E 'Starting rsky-ingester|EXIT-TRACE|CRITICAL' /tmp/ingester_debug.log | \
  awk '{print $1, $2, $NF}'
```

## Possible Root Causes (Ranked by Likelihood)

### 1. WebSocket Connection Closes Without Retry (HIGH)
**Symptom**: EXIT PATH D, E, or F with `Ok(Err(...))`
**Fix**: Review connection error handling in firehose.rs, backfill.rs, labeler.rs
**Check**: Does the infinite loop have any path that can `return`?

### 2. Redis Connection Lost (HIGH)
**Symptom**: Error logs mentioning Redis connection
**Fix**: Add Redis connection health checks and retry logic
**Check**: Is Redis connection tested before each operation?

### 3. Task Panic on Malformed Data (MEDIUM)
**Symptom**: FATAL PANIC messages with backtrace
**Fix**: Add proper error handling for data parsing
**Check**: Are all `.unwrap()` and `.expect()` calls safe?

### 4. OOM Kill (MEDIUM)
**Symptom**: No EXIT-TRACE after startup, container restart, `dmesg` shows OOM
**Fix**: Add memory limits, reduce batch sizes, implement backpressure
**Check**: Is memory usage growing over time?

### 5. Bug in Infinite Loop Logic (LOW)
**Symptom**: EXIT PATH with `Ok(Ok(()))`
**Fix**: Review loop structure, ensure no early returns
**Check**: Can `run()` function ever return Ok(())?

### 6. External Signal (LOW)
**Symptom**: Clean exit with no logs
**Fix**: Check for automation, health checks, manual intervention
**Check**: Is anything else managing the container?

## Success Criteria

After the fix is deployed:

- ✅ No `[EXIT-TRACE]` messages after initial startup
- ✅ No container restarts for 24+ hours
- ✅ Continuous processing shown in logs
- ✅ Memory usage stable
- ✅ All three ingester types running continuously

## Next Actions

1. **Deploy instrumented build to production**
2. **Capture full logs from at least one restart cycle**
3. **Analyze EXIT-TRACE messages to identify which task exits**
4. **Review the corresponding run() function for the bug**
5. **Implement fix based on root cause**
6. **Re-deploy and verify stability**

## Questions This Will Answer

1. **Which ingester type exits first?** (firehose, backfill, or labeler)
2. **Does it exit with Ok or Err?** (success vs error)
3. **What is the error if any?** (connection, Redis, parsing, etc.)
4. **Is it a panic or clean exit?** (panic hook vs normal return)
5. **Does it happen immediately or after some time?** (timing of EXIT-TRACE messages)
6. **Is the exit deterministic?** (same EXIT PATH each time or random)

## Reference: Code Structure

```rust
// ingester.rs main() structure with instrumentation:

match mode {
    "all" => {
        // [EXIT-TRACE] Running in 'all' mode
        spawn firehose_handle
        spawn backfill_handle
        spawn labeler_handle

        // [EXIT-TRACE] Entering tokio::select!
        tokio::select! {
            result = firehose_handle => {
                // [EXIT-TRACE] EXIT PATH D
                // [EXIT-TRACE] Firehose result: {:?}
                return Err(...)
            }
            result = backfill_handle => {
                // [EXIT-TRACE] EXIT PATH E
                return Err(...)
            }
            result = labeler_handle => {
                // [EXIT-TRACE] EXIT PATH F
                return Err(...)
            }
        }
    }
}

// Each run() function structure:
async fn run_firehose() -> Result<()> {
    for hostname in hosts {
        spawn task {
            ingester.run(hostname).await // <- Should NEVER return!
        }
    }

    // Wait for ALL tasks (should never complete)
    for task in tasks {
        task.await // <- If we reach here, a task exited!
    }

    Err(anyhow!("All tasks exited")) // <- This is what we'll see
}

// Each ingester's run() method:
pub async fn run(&self, hostname: String) -> Result<(), IngesterError> {
    loop { // <- INFINITE loop, should NEVER exit
        match self.run_connection(&hostname).await {
            Ok(()) | Err(_) => {
                // Sleep and retry
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
    // <- Code should NEVER reach here!
}
```

---

**Status**: Instrumentation added, ready for production deployment and log capture.
