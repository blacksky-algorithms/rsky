# rsky-ingester

Rust implementation of Bluesky AppView ingester services that subscribe to AT Protocol firehose and write events to Redis streams.

## Architecture

The ingester consists of three main components:

### 1. FirehoseIngester
- Subscribes to `com.atproto.sync.subscribeRepos` via WebSocket
- Converts firehose events to `StreamEvent` objects
- Writes to `firehose_live` Redis stream
- Implements backpressure monitoring
- Stores cursor position in Redis

### 2. BackfillIngester
- Calls `com.atproto.sync.listRepos` with pagination
- Creates `BackfillEvent` for each repo
- Writes to `repo_backfill` Redis stream
- Marks completion with special cursor `!ingester-done`

### 3. LabelerIngester
- Subscribes to `com.atproto.label.subscribeLabels`
- Converts label events to `LabelStreamEvent`
- Writes to `label_live` Redis stream

## Redis Streams

The ingester writes to the following Redis streams:

- `firehose_live` - Live firehose events from subscribeRepos
- `repo_backfill` - Repo references to be backfilled
- `label_live` - Label events from subscribeLabels

Cursors are stored as:
- `firehose_live:cursor:{hostname}`
- `repo_backfill:cursor:{hostname}`
- `label_live:cursor:{hostname}`

## Configuration

Configure via environment variables:

```bash
# Required
REDIS_URL=redis://localhost:6379

# Relay hosts (comma-separated)
RELAY_HOSTS=bsky.network

# Labeler hosts (comma-separated, optional)
LABELER_HOSTS=mod.bsky.app

# Ingester mode: all, firehose, backfill, or labeler
INGESTER_MODE=all

# Backpressure threshold
INGESTER_HIGH_WATER_MARK=100000

# Batch settings
INGESTER_BATCH_SIZE=500
INGESTER_BATCH_TIMEOUT_MS=1000

# Logging
RUST_LOG=info
```

## Running

```bash
# Run all ingesters
cargo run --bin ingester

# Run only firehose ingester
INGESTER_MODE=firehose cargo run --bin ingester

# Run only backfill ingester
INGESTER_MODE=backfill cargo run --bin ingester

# Run only labeler ingester
INGESTER_MODE=labeler cargo run --bin ingester
```

## Event Types

### StreamEvent
```rust
pub enum StreamEvent {
    Create { seq, time, did, commit, rev, collection, rkey, cid, record },
    Update { seq, time, did, commit, rev, collection, rkey, cid, record },
    Delete { seq, time, did, commit, rev, collection, rkey },
    Repo { seq, time, did, commit, rev },
    Account { seq, time, did, active, status },
    Identity { seq, time, did, handle },
}
```

### BackfillEvent
```rust
pub struct BackfillEvent {
    pub did: String,
    pub host: String,
    pub rev: String,
    pub status: Option<String>,
    pub active: bool,
}
```

## Backpressure

The ingester monitors Redis stream length every 5 seconds. If the stream length exceeds `INGESTER_HIGH_WATER_MARK` (default 100k), the ingester pauses for 5 seconds before retrying.

## Batching

Events are batched using the `Batcher` component which flushes either when:
- Batch size reaches `INGESTER_BATCH_SIZE` (default 500)
- Timeout of `INGESTER_BATCH_TIMEOUT_MS` (default 1000ms) is reached

## References

- [DATAPLANE_DEBUG.md](../DATAPLANE_DEBUG.md) - Detailed data flow documentation
- [DIVY.md](../DIVY.md) - Original backfill implementation notes
- `rsky-firehose` - WebSocket subscription patterns
- `rsky-relay` - Multi-subscription management patterns
