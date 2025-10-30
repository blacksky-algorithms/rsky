# CLAUDE.MD - rsky-ingester OOM Diagnosis Guide

## Critical Issue: Memory Exhaustion Despite Backpressure

**Problem**: The Rust ingester is hitting OOM (Out of Memory) kills despite backpressure being configured and detecting high water marks. Memory usage grows from 2.7GB to 7.3GB in 60 seconds, then container is killed by cgroup OOM killer.

**Expected Behavior**: When Redis stream length exceeds `INGESTER_HIGH_WATER_MARK` (100k), ingestion should PAUSE completely until the queue drains. Memory should remain stable.

**Actual Behavior**: Backpressure is detected (logs show "Backpressure: stream length 1564120 >= 100000"), but memory continues growing until OOM kill.

## Architecture Overview

```
WebSocket (Firehose) → FirehoseIngester → Batcher → Redis Stream
                              ↓                ↓
                         process_message   write_batch
                              ↓                ↓
                         StreamEvent      (backpressure check)
```

### Data Flow
1. **WebSocket Loop**: Reads binary messages from AT Protocol firehose
2. **Message Processing**: Parses CAR files, extracts IPLD records, converts to StreamEvents
3. **Batcher**: Collects events in batches (size=500, timeout=1000ms)
4. **Write Task**: Checks backpressure, writes batches to Redis

## Root Cause Hypothesis

**The unbounded channel in the batcher is accumulating events without limit.**

### Code Analysis

**File**: `rsky-ingester/src/batcher.rs`
```rust
pub fn new(
    max_size: usize,
    timeout_ms: u64,
) -> (mpsc::UnboundedSender<T>, mpsc::UnboundedReceiver<Vec<T>>) {
    let (tx, rx) = mpsc::unbounded_channel();  // ← UNBOUNDED!
    let (flush_tx, flush_rx) = mpsc::unbounded_channel();  // ← UNBOUNDED!
```

**File**: `rsky-ingester/src/firehose.rs` (line ~115)
```rust
// Read messages from WebSocket
while let Some(msg_result) = read.next().await {
    match msg_result {
        Ok(Message::Binary(data)) => match self.process_message(&data).await {
            Ok(events) => {
                for event in events {
                    if let Err(e) = batch_tx.send(event) {  // ← Never blocks!
                        error!("Failed to send event to batcher: {:?}", e);
                        break;
                    }
                }
            }
```

**The Problem**:
1. WebSocket loop reads messages continuously (4000+ events/sec)
2. Each message processed into StreamEvents (can be multiple events per message)
3. Events sent to `batch_tx` (unbounded channel) without blocking
4. Batcher's write task is PAUSED due to backpressure check
5. Events accumulate in memory in the unbounded channel
6. Memory grows: ~500 bytes per event × 1.5M events = 750MB+ just for events, plus CAR parsing overhead

## Diagnostic Steps for Claude Code

### 1. Verify Backpressure Detection
**Check**: Does backpressure detection work?
- Look at `firehose.rs` line ~90-97 in `write_task`
- Confirm `stream_len >= high_water_mark` triggers sleep
- **Status**: ✅ Working (logs confirm detection)

### 2. Identify Memory Accumulation Point
**Check**: Where is memory accumulating?
- `batcher.rs`: `rx: mpsc::UnboundedReceiver<T>` - events waiting to be batched
- `batcher.rs`: `flush_rx: mpsc::UnboundedReceiver<Vec<T>>` - batches waiting to be written
- **Hypothesis**: Events accumulate in `batch` field or internal queue

### 3. Verify Backpressure Propagation
**Critical Check**: Does backpressure stop WebSocket reading?
- `firehose.rs` line ~115: WebSocket loop has NO backpressure check
- WebSocket continues reading regardless of Redis stream length
- **Status**: ❌ NOT WORKING - this is the bug

### 4. Measure Event Rate vs Processing Rate
**Calculate**:
- Ingest rate: Check logs for events/second (estimated 4000+ from firehose)
- Write rate: When backpressure hits, write rate = 0
- Accumulation rate: 4000 events/sec × 60 sec = 240k events in memory
- Memory per event: ~4KB (after CAR parsing) = 960MB in 60 seconds

## Proposed Fixes

### Fix 1: Use Bounded Channels (Recommended)
Replace unbounded channels with bounded channels so backpressure naturally propagates.

**File**: `rsky-ingester/src/batcher.rs`
```rust
pub fn new(
    max_size: usize,
    timeout_ms: u64,
) -> (mpsc::Sender<T>, mpsc::UnboundedReceiver<Vec<T>>) {
    // Bounded channel with capacity = 2x batch size
    // This limits in-memory events to ~1000 (conservative)
    let (tx, rx) = mpsc::channel(max_size * 2);
    let (flush_tx, flush_rx) = mpsc::unbounded_channel();
    // ... rest of code
```

**Impact**:
- `batch_tx.send(event).await` will block when channel is full
- WebSocket reading pauses automatically
- Memory bounded to ~1000 events max

### Fix 2: Add Explicit Backpressure Check in WebSocket Loop

**File**: `rsky-ingester/src/firehose.rs`
```rust
// Before processing messages, check backpressure
let mut backpressure_check_interval = interval(Duration::from_secs(1));

while let Some(msg_result) = read.next().await {
    tokio::select! {
        _ = backpressure_check_interval.tick() => {
            // Check Redis stream length
            let stream_len: usize = conn.xlen(streams::FIREHOSE_LIVE).await?;
            if stream_len >= self.config.high_water_mark {
                warn!("Backpressure: pausing WebSocket reads");
                // Pause for 5 seconds
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        }
        msg = read.next() => {
            // Process message...
        }
    }
}
```

### Fix 3: Pause WebSocket Connection During Backpressure

**File**: `rsky-ingester/src/firehose.rs`
```rust
// Add a shared backpressure state
let backpressure_active = Arc::new(AtomicBool::new(false));

// In write_task, update the state
if stream_len >= high_water_mark {
    backpressure_active.store(true, Ordering::Relaxed);
    warn!("Backpressure active");
    tokio::time::sleep(Duration::from_secs(5)).await;
} else {
    backpressure_active.store(false, Ordering::Relaxed);
}

// In WebSocket loop, check before processing
if backpressure_active.load(Ordering::Relaxed) {
    tokio::time::sleep(Duration::from_millis(100)).await;
    continue;
}
```

## Testing Plan

### 1. Verify Fix Locally
```bash
# Set very low high water mark to trigger immediately
INGESTER_HIGH_WATER_MARK=100 \
REDIS_URL=redis://localhost:6379 \
INGESTER_RELAY_HOSTS=relay1.us-east.bsky.network \
cargo run --bin ingester
```

**Expected**: Memory stays stable, logs show backpressure pausing ingestion

### 2. Monitor Memory Usage
```bash
# In one terminal
docker stats rust-ingester

# In another, watch Redis
redis-cli XLEN firehose_live
```

**Expected**: When XLEN > 100, memory stops growing

### 3. Production Deployment
- Deploy with bounded channels (Fix 1)
- Monitor for 1 hour
- Check: No OOM kills, memory stable at <2GB
- Check: Redis stream length oscillates around high water mark

## Additional Improvements

### 1. Add Memory Metrics
**File**: `rsky-ingester/src/firehose.rs`
```rust
// Track in-memory events
let events_in_memory = Arc::new(AtomicUsize::new(0));

// Increment when sending to batcher
events_in_memory.fetch_add(1, Ordering::Relaxed);

// Decrement after writing to Redis
events_in_memory.fetch_sub(batch.len(), Ordering::Relaxed);

// Log periodically
info!("Events in memory: {}", events_in_memory.load(Ordering::Relaxed));
```

### 2. Add Circuit Breaker
If memory usage exceeds threshold, forcefully pause all ingestion:
```rust
// Check memory usage
let mem_info = sys_info::mem_info()?;
let mem_used_percent = (mem_info.total - mem_info.avail) * 100 / mem_info.total;

if mem_used_percent > 80 {
    error!("Memory usage critical: {}%, pausing ingestion", mem_used_percent);
    tokio::time::sleep(Duration::from_secs(30)).await;
}
```

### 3. Reduce Batch Size During Backpressure
```rust
let batch_size = if stream_len >= high_water_mark {
    100  // Smaller batches during backpressure
} else {
    500  // Normal batch size
};
```

## Similar Issues in Other Ingesters

### LabelerIngester (labeler.rs)
- Same architecture, same unbounded channel issue
- Apply Fix 1 (bounded channels)

### BackfillIngester (backfill.rs)
- Uses bounded channel in write task but unbounded in batcher
- Less critical (HTTP is naturally rate-limited)
- Still should apply Fix 1 for consistency

## Immediate Action Items

1. **CRITICAL**: Replace `mpsc::unbounded_channel()` with `mpsc::channel(capacity)` in `batcher.rs`
2. **HIGH**: Test locally with low high water mark
3. **HIGH**: Deploy to staging/production
4. **MEDIUM**: Add memory metrics
5. **MEDIUM**: Add circuit breaker
6. **LOW**: Consider streaming CAR parsing to reduce memory spikes

## Questions for Investigation

1. Why does CAR parsing consume so much memory? (each commit message ~4KB in memory)
2. Can we process messages without fully deserializing? (streaming CBOR decode)
3. Should we implement a separate backpressure mechanism for each relay host?
4. Should backpressure trigger connection close/reconnect instead of just pausing?

---

## Key Files to Review

### Rust (rsky)
- `~/Projects/rsky/rsky-indexer/src/indexing/mod.rs` - Main indexing logic
- `~/Projects/rsky/rsky-indexer/src/event.rs` - Event parsing
- `~/Projects/rsky/rsky-backfiller/src/main.rs` - Backfill processing
- `~/Projects/rsky/rsky-ingester/src/main.rs` - Firehose and Backfill Repo ingestion

### TypeScript (Reference)
- `~/Projects/atproto/packages/bsky/src/data-plane/server/indexer/stream.ts`
- `~/Projects/atproto/packages/bsky/src/data-plane/server/ingester/repo-backfiller.ts`
- `~/Projects/atproto/packages/bsky/src/data-plane/server/indexing/index.ts`
- `~/Projects/atproto/packages/bsky/src/data-plane/server/indexing/processor.ts`
- All files in `~/Projects/atproto/services/bsky`

Event Flow
```
┌─────────────────────────────────────────────────────────────────┐
│                    AT Protocol Relay                             │
│          (Firehose WebSocket + listRepos endpoint)               │
└────────────┬────────────────────────────────┬───────────────────┘
             │                                 │
             │ Firehose events                 │ Repo list
             ▼                                 ▼
    ┌────────────────┐              ┌───────────────────┐
    │ FirehoseIngester│              │ BackfillIngester │
    └────────┬───────┘              └─────────┬─────────┘
             │                                 │
             │ writes to                       │ writes to
             ▼                                 ▼
    ┌─────────────────┐              ┌──────────────────┐
    │ firehose_live   │              │ repo_backfill    │
    │ (Redis Stream)  │              │ (Redis Stream)   │
    └─────────────────┘              └────────┬─────────┘
                                               │
                                               │ consumed by
                                               ▼
                                     ┌───────────────────┐
                                     │ RepoBackfiller    │
                                     └─────────┬─────────┘
                                               │
                                               │ writes to
                                               ▼
                                     ┌────────────────────┐
                                     │ firehose_backfill  │
                                     │ (Redis Stream)     │
                                     └────────────────────┘

    ┌─────────────────┐              ┌────────────────────┐
    │ firehose_live   │              │ firehose_backfill  │
    └────────┬────────┘              └─────────┬──────────┘
             │                                  │
             │ consumed by                      │ consumed by
             └──────────┬───────────────────────┘
                        ▼
              ┌──────────────────────┐
              │  StreamIndexer(s)     │
              │  (Consumer Group)     │
              └──────────┬────────────┘
                         │
                         │ writes to
                         ▼
              ┌────────────────────────┐
              │   PostgreSQL           │
              │   (blacksky_bsky)      │
              └────────────────────────┘
```

**Summary**: The root cause is unbounded channels accumulating events in memory when write task is paused by backpressure. The WebSocket reading loop never pauses, so events accumulate until OOM. Fix by using bounded channels to propagate backpressure naturally.

**Remember**:
1. **The goal is functional equivalence with ZERO schema changes**
2. **No panics, no crash loops, no OOM errors**
3. When in doubt, copy TypeScript behavior exactly
4. Better to skip bad events than crash the entire system
5. Memory safety is not optional - it's required for production