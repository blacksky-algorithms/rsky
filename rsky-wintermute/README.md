`wintermute`: AT Protocol indexer for bsky app-view
========================================

Wintermute is a monolithic indexer that subscribes to AT Protocol relays, processes the firehose, backfills historical data, and writes to a PostgreSQL database compatible with the bsky app-view dataplane.

Wintermute combines three logical components (ingester, backfiller, indexer) into a single binary for simplified deployment. It uses Fjall (an LSM-tree embedded database) for high-throughput internal queues, avoiding external dependencies like Redis.

Features and design decisions:

- full-network indexing: processes all repos on the AT Protocol network
- automatic backfill: fetches historical data from PDSs via `com.atproto.sync.listRepos`
- label subscription: connects to labeler services to index labels
- parallel processing: independent queues for live events, backfill, and labels
- durable queues: Fjall-backed on-disk queues survive restarts
- dataplane compatible: writes to PostgreSQL schema expected by bsky app-view
- Prometheus metrics: exposes `/metrics` endpoint for monitoring
- graceful shutdown: drains in-flight work on SIGTERM/SIGINT

This tool is designed for operating a bsky app-view that needs to index the entire AT Protocol network (40M+ users, 15B+ records).

## Architecture

```
                    AT Protocol Relay
       (Firehose WebSocket + listRepos + Labels endpoint)
                            |
           +----------------+----------------+
           |                |                |
           v                v                v
    +-----------+    +-----------+    +-----------+
    | Firehose  |    | listRepos |    |  Labels   |
    +-----------+    +-----------+    +-----------+
           |                |                |
           |                v                |
           |         +-------------+         |
           |         |repo_backfill|         |
           |         |   (fjall)   |         |
           |         +------+------+         |
           |                |                |
           |                v                |
           |         +-----------+           |
           |         | Backfiller|           |
           |         +-----------+           |
           |                |                |
           |                v                |
           |        +--------------+         |
           |        |firehose_     |         |
           |        |backfill(fjall|         |
           |        +------+-------+         |
           |               |                 |
           |    +----------+                 |
           |    |                            |
           v    v                            v
    +------------------+            +------------------+
    | Inline Indexing  |            | Inline Indexing  |
    | (firehose+backfill)          |    (labels)      |
    +--------+---------+            +--------+---------+
             |                               |
             v                               v
            +---------------------------+
            |        PostgreSQL         |
            |   (bsky dataplane schema) |
            +---------------------------+
```

**Data flow:**
- **Firehose (live)**: Events are parsed and indexed inline (directly to PostgreSQL, no queue)
- **Labels (live)**: Events are parsed and indexed inline (directly to PostgreSQL, no queue)
- **Backfill**: DIDs queued to `repo_backfill`, backfiller fetches CARs, records queued to `firehose_backfill`, then indexed

## Quick Start

```bash
# Build wintermute
cargo build --release --package rsky-wintermute

# Run wintermute (requires PostgreSQL with bsky schema)
RELAY_HOSTS=bsky.network \
DATABASE_URL=postgresql://user:pass@localhost:5432/bsky \
RUST_LOG=info \
./target/release/wintermute
```

## Configuration

### Required Environment Variables

| Variable | Description                                                                                                     |
|----------|-----------------------------------------------------------------------------------------------------------------|
| `RELAY_HOSTS` | Comma-separated relay hosts (e.g., `bsky.network` or `relay1.us-east.bsky.network,relay1.us-west.bsky.network`) |
| `DATABASE_URL` | PostgreSQL connection string                                                                                    |

### Optional Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `LABELER_HOSTS` | (empty) | Comma-separated labeler hosts for label subscription |
| `METRICS_PORT` | `9090` | Port for Prometheus metrics endpoint |
| `RUST_LOG` | (none) | Log level (`error`, `warn`, `info`, `debug`, `trace`) |
| `INDEXER_WORKERS` | `16` | Concurrent index workers per queue |
| `INDEXER_BATCH_SIZE` | `1000` | Records per batch (test only) |
| `BACKFILLER_WORKERS` | `32` | Concurrent repo fetch workers |
| `BACKFILLER_BATCH_SIZE` | `1000` | Repos to dequeue per batch |
| `BACKFILLER_OUTPUT_HIGH_WATER_MARK` | `100000` | Max records in firehose_backfill before backpressure |
| `BACKFILLER_TIMEOUT_SECS` | `120` | Timeout for fetching repo CAR from PDS |
| `INLINE_CONCURRENCY` | `100` | Concurrent inline indexing tasks for firehose events |
| `DB_POOL_SIZE` | `20` | Connections per pool (4 pools: firehose, labels, indexer, backfiller) |

## Utilities

### queue_backfill

Manually queue DIDs for backfill from various sources:

```bash
# Queue DIDs from a CSV file
./target/release/queue_backfill csv --file dids.csv

# Queue all repos from a specific PDS
./target/release/queue_backfill pds --host blacksky.app

# Queue specific DIDs
./target/release/queue_backfill dids --did did:plc:abc123 --did did:plc:def456

# Show queue status
./target/release/queue_backfill status
```

## Queues

Wintermute uses Fjall for durable backfill queues:

| Queue | Purpose |
|-------|---------|
| `repo_backfill` | DIDs awaiting full repo fetch |
| `firehose_backfill` | Records extracted from backfilled repos |

**Inline processing (no queue):**
- **Firehose live events**: Parsed and indexed directly to PostgreSQL with concurrent tasks
- **Label live events**: Parsed and indexed directly to PostgreSQL with concurrent tasks

**Cursor state:** Stored in PostgreSQL `sub_state` table, not Fjall

Backfill uses semaphore-controlled concurrency with backpressure. Live events are never blocked by backfill processing.

## Indexed Record Types

Wintermute indexes all standard bsky record types:

- `app.bsky.feed.post` - Posts
- `app.bsky.feed.like` - Likes
- `app.bsky.feed.repost` - Reposts
- `app.bsky.graph.follow` - Follows
- `app.bsky.graph.block` - Blocks
- `app.bsky.actor.profile` - Profiles
- `app.bsky.feed.generator` - Feed generators
- `app.bsky.graph.list` - Lists
- `app.bsky.graph.listitem` - List items
- `app.bsky.graph.listblock` - List blocks
- `app.bsky.graph.starterpack` - Starter packs
- `app.bsky.labeler.service` - Labeler services
- `app.bsky.feed.threadgate` - Thread gates
- `app.bsky.feed.postgate` - Post gates
- `app.bsky.verification.proof` - Verification proofs
- `chat.bsky.actor.declaration` - Chat declarations
- `app.bsky.notification.declaration` - Notification declarations
- `app.bsky.actor.status` - Actor status

All records are also stored in the generic `record` table with full JSON.

## Metrics

Prometheus metrics are exposed at `http://localhost:9090/metrics`:

- `ingester_firehose_events_total` - Events received by stream type
- `ingester_firehose_live_length` - Current firehose_live queue size
- `ingester_firehose_backfill_length` - Current firehose_backfill queue size
- `ingester_label_live_length` - Current label_live queue size
- `ingester_websocket_connections` - Active WebSocket connections
- `ingester_errors_total` - Ingestion errors by type
- `indexer_records_processed_total` - Total records processed
- `indexer_records_failed_total` - Failed record indexing
- `indexer_stale_writes_skipped_total` - Skipped stale writes (older rev)
- `indexer_post_events_total` - Posts indexed
- `indexer_like_events_total` - Likes indexed
- `indexer_follow_events_total` - Follows indexed
- `indexer_repost_events_total` - Reposts indexed
- `indexer_block_events_total` - Blocks indexed
- `indexer_profile_events_total` - Profiles indexed
- `backfiller_repos_processed_total` - Repos backfilled
- `backfiller_repos_failed_total` - Failed repo backfills
- `backfiller_records_extracted_total` - Records extracted from repos

## Operations

### Cursor Management

Firehose and label cursors are stored in the PostgreSQL `sub_state` table. On restart, wintermute resumes from the last saved cursor position. Cursors are saved every 20 events.

### Backpressure

The backfiller implements backpressure via `BACKFILLER_OUTPUT_HIGH_WATER_MARK`. When the `firehose_backfill` queue exceeds this threshold, the backfiller pauses repo fetching until the indexer drains the queue. Live events are never affected by backpressure.

### Handle Resolution

Handles are resolved asynchronously after initial indexing. Actors with NULL handles are prioritized and re-checked every hour. Valid handles are re-verified every 24 hours.

### Graceful Shutdown

On SIGTERM or SIGINT, wintermute:
1. Stops accepting new events
2. Drains all in-flight work
3. Saves cursor positions
4. Exits cleanly

### Recovery

Fjall queues are durable and survive crashes. On restart, wintermute:
1. Resumes firehose from saved cursor
2. Continues processing queued backfill work
3. Reprocesses any in-flight records that weren't acknowledged

## Requirements

### System Requirements

- **Memory**: 8GB minimum, 32GB+ recommended for full network indexing
- **Storage**: 100GB+ for Fjall queues during backfill
- **CPU**: 8+ cores recommended for parallel processing

### PostgreSQL

Requires a PostgreSQL database with the bsky dataplane schema. Tables include:
- `actor`, `profile`, `profile_agg`
- `post`, `post_agg`
- `like`, `repost`, `follow`
- `actor_block`, `list`, `list_item`, `list_block`
- `feed_generator`, `labeler`, `starter_pack`
- `thread_gate`, `post_gate`
- `notification`, `label`
- `record`, `sub_state`
- `verification`

## License

rsky-wintermute is released under the [Apache License 2.0](../LICENSE).
