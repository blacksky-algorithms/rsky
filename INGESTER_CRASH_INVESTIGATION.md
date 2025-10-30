# rsky-ingester Production Crash Investigation

## Incident Summary

- **Time**: 2025-10-30 06:56:52 to ~06:57:21 (29 seconds runtime)
- **Behavior**: Container started, ran for ~29 seconds, then restarted
- **Current Status**: Container running successfully after restart
- **Configuration**:
  - 2 relay hosts (us-east, us-west)
  - 1 labeler host (atproto.africa)
  - High water mark: 1,000,000 messages

## Code Analysis

### Expected Behavior

All three ingester types are designed to run forever:

```rust
// firehose.rs, backfill.rs, labeler.rs
pub async fn run(&self, hostname: String) -> Result<(), IngesterError> {
    loop {
        match self.run_connection(&hostname).await {
            Ok(()) | Err(_) => {
                // Sleep and retry - NEVER exits
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
```

### Crash Detection Logic

In `ingester.rs`, each ingester type is spawned as a tokio task:

```rust
let task = tokio::spawn(async move {
    if let Err(e) = ingester_clone.run(hostname_clone.clone()).await {
        error!("CRITICAL: ... exited with error: {:?}", e);
    } else {
        error!("CRITICAL: ... exited successfully but should never exit!");
    }
});
```

Then `run_firehose`/`run_backfill`/`run_labeler` wait for ALL spawned tasks:

```rust
for (i, task) in tasks.into_iter().enumerate() {
    match task.await {
        Ok(()) => {
            error!("CRITICAL: task {} completed unexpectedly", i);
        }
        Err(e) => {
            error!("CRITICAL: task {} panicked: {:?}", i);
        }
    }
}
// After ALL tasks complete:
Err(anyhow::anyhow!("CRITICAL: All tasks exited"))
```

Finally, the main function's `tokio::select!` waits for any ingester type to return:

```rust
tokio::select! {
    result = firehose_handle => {
        error!("Firehose ingester exited: {:?}", result);
        return Err(anyhow::anyhow!("CRITICAL: Firehose ingester exited unexpectedly"));
    }
    result = backfill_handle => {
        error!("Backfill ingester exited: {:?}", result);
        return Err(anyhow::anyhow!("CRITICAL: Backfill ingester exited unexpectedly"));
    }
    result = labeler_handle => {
        error!("Labeler ingester exited: {:?}", result);
        return Err(anyhow::anyhow!("CRITICAL: Labeler ingester exited unexpectedly"));
    }
}
```

**Critical Insight**: For the process to exit, ALL tasks for one ingester type must complete, which should be impossible given the infinite loops.

## Observed Logs (Partial)

Last logs shown before crash (06:56:52 to 06:56:59):

```
✅ All 3 ingester types started successfully
✅ Connections established:
   - FirehoseIngester: us-east (cursor 6328157203), us-west (cursor 6007487555)
   - BackfillIngester: us-east (cursor 36724), us-west (cursor 20971)
   - LabelerIngester: atproto.africa (cursor 27045591)
✅ Backpressure working (stream length 3.5M > high water mark)
✅ Error handling working (500 errors logged with retry after 30s)
```

**Gap**: Missing ~22 seconds of logs between 06:56:59 and crash at ~06:57:21

## Potential Crash Causes

### 1. Task Completion (Most Likely)

One or more spawned tasks completed unexpectedly, causing their parent function to return an error.

**Evidence**:
- No panic messages in partial logs
- Clean restart (suggests controlled exit)
- All components initialized successfully

**Possible Triggers**:
- WebSocket connection closed unexpectedly
- Redis connection lost
- Unhandled error in message processing
- Bug in retry logic

### 2. OOM Kill

Process exceeded memory limits (6GB configured).

**Evidence Against**:
- Would see OOM messages in system logs
- Quick crash (29s) suggests logic error, not memory leak
- Memory leaks take longer to manifest

### 3. Panic Bypassing Hook

Panic occurred in a way that bypassed the panic hook.

**Evidence Against**:
- Panic hook should catch all panics
- No panic traces in partial logs
- Error handling appears to be working

### 4. External Kill

Process killed by external signal (SIGTERM, SIGKILL).

**Evidence Against**:
- No indication of manual intervention
- Docker restart suggests error exit, not signal

## Diagnostic Plan

### Immediate Actions (Production)

1. **Get Full Logs from Failed Run**
   ```bash
   # On production server
   docker logs rust-ingester --since 2025-10-30T06:56:50 --until 2025-10-30T06:57:25
   ```

2. **Check for OOM Kill**
   ```bash
   docker inspect rust-ingester | grep -i oom
   ```

3. **Monitor Current Run**
   ```bash
   # Check if it's stable now
   docker ps | grep rust-ingester
   docker logs rust-ingester -f | grep -E "CRITICAL|ERROR|panic"
   ```

4. **Check Redis/Relay Health**
   ```bash
   # Test connectivity
   redis-cli -u redis://redis:6379 PING
   curl -I https://relay1.us-east.bsky.network
   curl -I https://relay1.us-west.bsky.network
   curl -I https://atproto.africa
   ```

### Code Fixes to Consider

1. **Add Detailed Exit Logging**

   In `ingester.rs`, add more context when tasks complete:

   ```rust
   let task = tokio::spawn(async move {
       let start = std::time::Instant::now();
       let result = ingester_clone.run(hostname_clone.clone()).await;
       let duration = start.elapsed();

       error!(
           "CRITICAL: Ingester for {} exited after {:?} with result: {:?}\n\
            This should NEVER happen. Stack trace:\n{:?}",
           hostname_clone,
           duration,
           result,
           std::backtrace::Backtrace::capture()
       );
   });
   ```

2. **Add Heartbeat Logging**

   In each ingester's `run()` function:

   ```rust
   let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(60));

   loop {
       tokio::select! {
           _ = heartbeat_interval.tick() => {
               info!("Heartbeat: {} still running, processed {} events",
                     hostname, event_count);
           }
           result = self.run_connection(&hostname) => {
               // Existing error handling
           }
       }
   }
   ```

3. **Add Connection Health Checks**

   Before reconnecting, verify Redis/relay is accessible:

   ```rust
   async fn check_health(&self) -> bool {
       // Test Redis connection
       match self.redis_client.get_multiplexed_async_connection().await {
           Ok(_) => true,
           Err(e) => {
               error!("Redis health check failed: {}", e);
               false
           }
       }
   }
   ```

## Hypothesis

Based on the code structure and observed behavior, the most likely scenario is:

**One of the WebSocket connections (firehose or labeler) closed unexpectedly**, causing `run_connection()` to return `Ok(())` at line 165 (firehose.rs) or line 155 (labeler.rs). This should trigger a reconnect via the outer loop, but if there's a subtle bug or race condition, it could cause the function to exit.

**Next Step**: Need to see the full logs from 06:56:59 to 06:57:21 to identify which component exited and why.

## Success Criteria for Fix

- ✅ Zero restarts for 24+ hours
- ✅ Clear error messages if crash does occur
- ✅ Automatic recovery from transient failures
- ✅ Heartbeat logs every 60 seconds showing healthy operation

## Questions to Answer

1. Which ingester type exited first? (firehose, backfill, or labeler)
2. Was it a clean exit (Ok(())) or error exit (Err(...))?
3. Which hostname was affected? (us-east, us-west, or atproto.africa)
4. What was the last message processed before exit?
5. Is the current run stable, or has it restarted again?
