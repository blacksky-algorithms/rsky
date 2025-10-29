# rsky-backfiller

Rust implementation of Bluesky AppView repo backfiller that fetches full repos and queues records for indexing.

## Architecture

The backfiller reads from `repo_backfill` Redis stream, fetches complete repos, verifies signatures, and writes record events to the `firehose_backfill` stream for indexing.

### Main Component: RepoBackfiller

- Reads from `repo_backfill` stream using consumer groups
- Fetches repos via `com.atproto.sync.getRepo`
- Parses CAR files using `iroh-car`
- Verifies repo signatures using `rsky-repo`
- Extracts and writes record events to `firehose_backfill` stream
- Handles backpressure on output stream
- Concurrent processing with configurable concurrency

## Flow

1. Read BackfillEvent from `repo_backfill` stream
2. Fetch repo CAR file via HTTP
3. Parse CAR and verify repo:
   - Load commit and MST from blocks
   - Resolve DID to get signing key
   - Verify commit signature
4. Extract all records from repo
5. Write StreamEvent (create) for each record to `firehose_backfill`
6. Write StreamEvent (repo) after each chunk
7. ACK and delete message from input stream

## Configuration

Configure via environment variables:

```bash
# Required
REDIS_URL=redis://localhost:6379
BACKFILLER_CONSUMER=backfiller_1

# Optional
BACKFILLER_BACKFILL_STREAM=repo_backfill
BACKFILLER_FIREHOSE_STREAM=firehose_backfill
BACKFILLER_GROUP=repo_backfill_group
BACKFILLER_CONCURRENCY=2
BACKFILLER_FIREHOSE_STREAM_HIGH_WATER_MARK=100000

# Logging
RUST_LOG=info,rsky_backfiller=debug
```

## Running

```bash
# Run backfiller
BACKFILLER_CONSUMER=backfiller_1 cargo run --bin backfiller

# Multiple instances for scaling
BACKFILLER_CONSUMER=backfiller_1 cargo run --bin backfiller &
BACKFILLER_CONSUMER=backfiller_2 cargo run --bin backfiller &
```

## Consumer Groups

The backfiller uses Redis consumer groups for distributed processing:

- **Consumer Group**: Multiple backfiller instances can read from the same stream
- **Consumer Name**: Each instance must have a unique consumer name
- **Pending Messages**: Failed messages remain in pending state for retry
- **ACK**: Messages are acknowledged and deleted after successful processing

## Concurrency

Repos are processed concurrently:

- `BACKFILLER_CONCURRENCY` controls max concurrent tasks (default: 2)
- Each repo is processed in its own tokio task
- Backpressure is applied when concurrency limit is reached

## Backpressure

Output stream length is monitored:

- Checks `firehose_backfill` stream length before processing
- Waits when length exceeds high water mark (default: 100,000)
- Prevents overwhelming downstream indexers

## Special Features

### Repo Verification

- Verifies repo commit signatures using resolved DID keys
- Ensures DID matches between repo and request
- Uses `rsky-repo` for CAR parsing and verification
- Uses `rsky-identity` for DID resolution

### Chunking

Records are written in chunks of 500:

- Reduces memory usage for large repos
- Provides progress feedback
- Each chunk ends with a repo event

### Event Format

All events are written with `seq: -1` (SEQ_BACKFILL) to indicate backfill source.

```json
{
  "type": "create",
  "seq": -1,
  "time": "2025-01-01T00:00:00Z",
  "did": "did:plc:...",
  "commit": "bafyrei...",
  "rev": "3lhm...",
  "collection": "app.bsky.feed.post",
  "rkey": "3km...",
  "cid": "bafyrei...",
  "record": { ... }
}
```

## Error Handling

Errors are logged but messages remain in pending state:

- Repo fetch failures (404, timeout, etc.)
- CAR parsing errors
- Signature verification failures
- Identity resolution failures
- Bad message format (ACKed and deleted)

## Dependencies

- **redis** - Stream operations
- **reqwest** - HTTP client for repo fetching
- **rsky-repo** - CAR parsing and verification
- **rsky-identity** - DID resolution
- **iroh-car** - CAR file format handling

## References

- [repo-backfiller.ts](../atproto/packages/bsky/src/data-plane/server/ingester/repo-backfiller.ts) - TypeScript reference
- [backfiller.js](../atproto/services/bsky/backfiller.js) - Service reference
- [rsky-repo](../rsky-repo) - Repo verification patterns
- [rsky-ingester](../rsky-ingester) - Event streaming patterns
