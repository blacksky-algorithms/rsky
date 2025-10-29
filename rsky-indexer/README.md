# rsky-indexer

Rust implementation of Bluesky AppView indexer services that read from Redis streams and index records into PostgreSQL.

## Architecture

The indexer consists of two main components:

### 1. StreamIndexer
- Reads from `firehose_live` and `firehose_backfill` Redis streams
- Uses consumer groups for distributed processing
- Processes events concurrently with configurable concurrency
- Indexes records into PostgreSQL using collection-specific plugins
- Handles create/update/delete operations
- Tracks commit state and actor status

### 2. LabelIndexer
- Reads from `label_live` Redis stream
- Indexes label events into PostgreSQL
- Handles label negation (deletion)

## Plugin System

The indexer uses a plugin architecture for handling different record types. All 18 Bluesky record types are supported:

### Feed & Social Plugins
- **PostPlugin** - `app.bsky.feed.post` - Posts and replies
- **LikePlugin** - `app.bsky.feed.like` - Likes
- **RepostPlugin** - `app.bsky.feed.repost` - Reposts
- **PostGatePlugin** - `app.bsky.feed.postgate` - Post visibility rules
- **ThreadGatePlugin** - `app.bsky.feed.threadgate` - Thread reply rules
- **FeedGeneratorPlugin** - `app.bsky.feed.generator` - Custom feed generators

### Graph Plugins
- **FollowPlugin** - `app.bsky.graph.follow` - Follows
- **BlockPlugin** - `app.bsky.graph.block` - Blocks
- **ListPlugin** - `app.bsky.graph.list` - Lists (moderation/curated)
- **ListItemPlugin** - `app.bsky.graph.listitem` - List membership
- **ListBlockPlugin** - `app.bsky.graph.listblock` - List-based blocks
- **StarterPackPlugin** - `app.bsky.graph.starterpack` - Starter packs

### Actor Plugins
- **ProfilePlugin** - `app.bsky.actor.profile` - User profiles
- **StatusPlugin** - `app.bsky.actor.status` - Actor status
- **VerificationPlugin** - `app.bsky.actor.verification` - Identity verification

### System Plugins
- **LabelerPlugin** - `app.bsky.labeler.service` - Labeler services
- **ChatDeclarationPlugin** - `chat.bsky.actor.declaration` - Chat declarations
- **NotifDeclarationPlugin** - `app.bsky.notification.declaration` - Notification preferences

Each plugin implements:
- `insert()` - Create a new record
- `update()` - Update an existing record
- `delete()` - Delete a record

## Database Schema

The indexer expects the following core tables:

```sql
-- Generic record table
CREATE TABLE record (
    uri TEXT PRIMARY KEY,
    cid TEXT NOT NULL,
    did TEXT NOT NULL,
    json JSONB NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL
);

-- Actor sync state
CREATE TABLE actor_sync (
    did TEXT PRIMARY KEY,
    commit_cid TEXT NOT NULL,
    repo_rev TEXT NOT NULL,
    last_seen TIMESTAMPTZ NOT NULL
);

-- Actor table
CREATE TABLE actor (
    did TEXT PRIMARY KEY,
    handle TEXT UNIQUE,
    indexed_at TIMESTAMPTZ,
    upstream_status TEXT
);

-- Collection-specific tables (examples)
CREATE TABLE post (
    uri TEXT PRIMARY KEY,
    cid TEXT NOT NULL,
    creator TEXT NOT NULL,
    text TEXT NOT NULL,
    reply_root TEXT,
    reply_parent TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE like (
    uri TEXT PRIMARY KEY,
    cid TEXT NOT NULL,
    creator TEXT NOT NULL,
    subject_uri TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE follow (
    uri TEXT PRIMARY KEY,
    cid TEXT NOT NULL,
    creator TEXT NOT NULL,
    subject_did TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE label (
    src TEXT NOT NULL,
    uri TEXT NOT NULL,
    cid TEXT,
    val TEXT NOT NULL,
    cts TEXT NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (src, uri, val)
);

-- Notification table
CREATE TABLE notification (
    id SERIAL PRIMARY KEY,
    did TEXT NOT NULL,
    record_uri TEXT NOT NULL,
    record_cid TEXT NOT NULL,
    author TEXT NOT NULL,
    reason TEXT NOT NULL,
    reason_subject TEXT,
    sort_at TEXT NOT NULL
);
CREATE INDEX notification_did_sort_at ON notification (did, sort_at DESC);
```

## Configuration

Configure via environment variables:

```bash
# Required
REDIS_URL=redis://localhost:6379
DATABASE_URL=postgres://user:pass@localhost/bsky

# Indexer settings
INDEXER_STREAMS=firehose_live,firehose_backfill
INDEXER_GROUP=firehose_group
INDEXER_CONSUMER=indexer_1
INDEXER_CONCURRENCY=10
INDEXER_BATCH_SIZE=100

# Indexer mode: all, stream, or label
INDEXER_MODE=all

# Logging
RUST_LOG=info
```

## Running

```bash
# Run all indexers
cargo run --bin indexer

# Run only stream indexer
INDEXER_MODE=stream cargo run --bin indexer

# Run only label indexer
INDEXER_MODE=label cargo run --bin indexer

# Multiple instances for scaling
INDEXER_CONSUMER=indexer_1 cargo run --bin indexer &
INDEXER_CONSUMER=indexer_2 cargo run --bin indexer &
```

## Consumer Groups

The indexer uses Redis consumer groups for distributed processing:

- **Consumer Group**: Multiple indexer instances can read from the same stream
- **Consumer Name**: Each instance must have a unique consumer name
- **Pending Messages**: Failed messages remain in pending state for retry
- **ACK**: Messages are acknowledged and deleted after successful processing

## Concurrency

Events are processed concurrently using a semaphore to limit parallelism:

- `INDEXER_CONCURRENCY` controls max concurrent tasks (default: 10)
- Each message is processed in its own tokio task
- Backpressure is applied when concurrency limit is reached

## Event Processing

### StreamEvent Types

1. **Create/Update** - Index a record
   - Insert into generic `record` table
   - Call collection-specific plugin insert/update
   - Update commit state (unless backfill)
   - Index handle if profile

2. **Delete** - Delete a record
   - Tombstone in `record` table
   - Call collection-specific plugin delete
   - Update commit state (unless backfill)

3. **Repo** - Update commit state
   - Update `actor_sync` table with commit CID and rev

4. **Account** - Update actor status
   - Update `actor` table with active/status
   - Index handle

5. **Identity** - Update actor handle
   - Resolve DID to handle
   - Update handle mapping

### Label Processing

- **Insert/Update**: Insert or update label in `label` table
- **Negation**: Delete label if `neg` is true

## Special Handling

### Backfill Events

Events with `seq == -1` are backfill events:
- Skip `setCommitLastSeen()` to avoid thrashing
- Still indexed normally into tables

### Duplicate Handling

- Uses `ON CONFLICT DO NOTHING` for idempotency
- Returns early if record already exists
- Prevents duplicate processing

## Metrics

(Future) The indexer tracks:
- `processed` - Total messages processed
- `failed` - Total messages failed
- `waiting` - Messages in queue
- `running` - Messages being processed

## References

- [DATAPLANE_DEBUG.md](../DATAPLANE_DEBUG.md) - Data flow documentation
- `atproto/packages/bsky/src/data-plane/server/indexer` - TypeScript reference
- `atproto/packages/bsky/src/data-plane/server/indexing` - Plugin patterns
