`wintermute`: AT Protocol indexer for bsky app-view
========================================

Wintermute is a monolithic indexer that subscribes to AT Protocol relays, processes the firehose, backfills historical data, and writes to a PostgreSQL database compatible with the bsky app-view dataplane.

Wintermute combines three logical components (ingester, backfill, indexer) into a single binary for simplified deployment. It uses Fjall (an LSM-tree embedded database) for the live-event queue, avoiding external dependencies like Redis; backfill writes to PostgreSQL directly.

Features and design decisions:

- full-network indexing: processes all repos on the AT Protocol network
- full-network backfill: fetches archives directly from Bluesky's PDS fleet and everything else from [hubble](https://hubble.microcosm.blue), the public mirror, under per-host rate budgets
- label subscription: connects to labeler services to index labels
- parallel processing: independent queues for live events, backfill, and labels
- durable queues: Fjall-backed on-disk queues survive restarts
- dataplane compatible: writes to PostgreSQL schema expected by bsky app-view
- Prometheus metrics: exposes `/metrics` endpoint for monitoring
- graceful shutdown: drains in-flight work on SIGTERM/SIGINT

This tool is designed for operating a bsky app-view that needs to index the entire AT Protocol network (40M+ users, 15B+ records).

## Architecture

```
       AT Protocol Relay                    PDS hosts (mushrooms)      hubble
  (firehose + listHosts + labels)          listRepos + getRepo     listRepos + getRepo
            |                                       |                     |
   +--------+--------+                              +----------+----------+
   |                 |                                         |
   v                 v                                         v
+-----------+  +-----------+                          +-----------------+
| Firehose  |  |  Labels   |                          |    Backfill     |
+-----------+  +-----------+                          | discover        |
   |                 |                                | enumerate       |
   v                 |                                | fetch per host  |
+-------------+      |                                | (rate + demand  |
|firehose_live|      |                                |  gated)         |
|  (fjall)    |      |                                +--------+--------+
+------+------+      |                                         |
       |             |                                         | COPY batches
       v             v                                         v
+---------------------------+                        +-------------------+
|   Indexer (live, labels)  |                        | backfill_state    |
+-------------+-------------+                        | (sqlite: per-repo |
              |                                      |  rev + per-host)  |
              v                                      +-------------------+
            +---------------------------+
            |        PostgreSQL         |
            |   (bsky dataplane schema) |
            +---------------------------+
```

**Data flow:**
- **Firehose (live)**: Events are parsed into the `firehose_live` queue and indexed in sharded batches
- **Labels (live)**: Events are parsed and indexed directly to PostgreSQL
- **Backfill**: hosts are discovered from the relay's `listHosts`; each direct host (by default Bluesky's `*.host.bsky.network` fleet) is enumerated with its own `listRepos`, then hubble is enumerated for everything else. Archives are fetched per source under a per-host rate budget, parsed, and written to PostgreSQL through the bulk COPY path -- only while the writers have room. Per-repo state (`rev` indexed, attempts, cooldown, source) lives in a SQLite file, so re-enumeration only creates work for repos that moved.

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
| `INLINE_CONCURRENCY` | `100` | Concurrent inline indexing tasks for label events |
| `DB_POOL_SIZE` | `20` | Connections per pool (ingester, labels, indexer live, indexer labels) |
| `RECORD_COLLECTION_ALLOWLIST` | (legacy `app.bsky.,chat.bsky.` for backfill) | Comma-separated NSID prefixes to index |
| `RECORD_SKIP_BOILERPLATE` | `false` | Skip `record` rows for like/repost/follow/block |
| `LIVE_AGGREGATES` | `true` | Update `post_agg`/`profile_agg` inline on the live path |
| `IDENTITY_EVENT_CONCURRENCY` | `64` | Concurrent `#identity` / `#account` / `#sync` tasks (each resolves a DID and handle over the network). When all permits are busy the event is shed and counted in `ingester_identity_tasks_shed_total`; the handle sweep re-verifies the account within a day |
| `IDENTITY_EVENT_TIMEOUT_SECS` | `15` | Deadline for one such task; expiries are counted in `ingester_identity_task_timeouts_total` |

### Memory Environment Variables

The Fjall store holds short-lived queues (`firehose_live`, `label_live`, cursors): entries are written once and deleted seconds later, so it lives almost entirely in memtables and the block cache only fills when the indexer falls behind. The defaults are sized for that, not for a general-purpose LSM database. The earlier defaults (32 GB cache, 256 MB memtables, 2 GB write buffer) date from when Fjall also held the repo backfill queue, which no longer exists.

| Variable | Default | Description |
|----------|---------|-------------|
| `FJALL_CACHE_SIZE_GB` | `4` | Block-cache ceiling. Only fills as segments are read, i.e. when the live queue has spilled past the memtable. Raise it on large hosts if `ingester_firehose_live_length` is routinely in the millions |
| `FJALL_MEMTABLE_MB` | `64` | Per-partition memtable flush threshold (Fjall recommends 8-64 MiB). A partition can transiently hold several memtables while flushes drain, so this multiplies |
| `FJALL_WRITE_BUFFER_SIZE_GB` | `1` | Total memtable bytes across all partitions before Fjall stalls writers. Never reached in steady state; a backstop if flushing falls behind |
| `MIMALLOC_PURGE_DELAY` | (allocator default) | `0` makes mimalloc return freed pages to the OS promptly instead of retaining them for reuse. Measured to lower steady-state RSS materially; set it in the service unit on memory-tight hosts |

Resident memory is exported as `wintermute_rss_bytes` / `wintermute_rss_peak_bytes` (from `/proc/self/status`), alongside `wintermute_fjall_write_buffer_bytes`, so it can be watched without a shell on the box. On startup the daemon deletes the orphaned `repo_backfill` partition if a store from the previous design still carries it; Fjall would otherwise keep its segment metadata resident and compact it forever.

### Backfill Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BACKFILL_MODE` | `off` | `off`, `hubble` (everything via hubble), `direct` (only direct hosts), `hybrid` (direct hosts directly, the rest via hubble) |
| `BACKFILL_SINK` | `postgres` | `null` fetches and parses without writing, to measure the fetch side; its completions are recorded as dry-run and re-queued by the next writing drain |
| `BACKFILL_STATE_DB` | `backfill_state.sqlite` | Per-repo / per-host state file (relative to the working directory) |
| `BACKFILL_RELAY` | first of `RELAY_HOSTS` | Relay whose `listHosts` discovers PDS hosts |
| `BACKFILL_DIRECT_HOSTS` | `*.host.bsky.network` | Comma-separated suffix globs or exact hosts to fetch directly |
| `BACKFILL_EXTRA_HOSTS` | (empty) | Hosts to fetch directly even if the relay does not list them (e.g. your own PDS) |
| `BACKFILL_BSKY_RPS` / `BACKFILL_BSKY_CONCURRENCY` | `10` / `10` | Per-mushroom request rate and in-flight fetches (the PDS global limiter is 3000 req / 5 min per IP) |
| `BACKFILL_PDS_RPS` / `BACKFILL_PDS_CONCURRENCY` | `3` / `6` | Per-host budget for any other direct host; concurrency steps down 3/4 at a time under transient errors and recovers |
| `BACKFILL_HUBBLE_URL` | `https://hubble.microcosm.blue` | |
| `BACKFILL_HUBBLE_RPS` / `BACKFILL_HUBBLE_CONCURRENCY` | `8` / `4` | hubble serves ~119 repos/s at concurrency 16 from nyc3 with a ~28 MB/s byte ceiling; stay well under it |
| `BACKFILL_USER_AGENT` | `rsky-wintermute/... (+https://blacksky.app; contact)` | Sent on every request; hubble requires a contact |
| `BACKFILL_MAX_WORKERS` | `128` | Concurrent per-source fetch workers |
| `BACKFILL_MAX_INFLIGHT` | `8 * CPUs` | Archives downloading or parsing at once across all sources; parsing holds a whole repo in memory and costs a core, so this is the backfill's CPU and memory budget |
| `BACKFILL_WRITERS` | `4` | Concurrent COPY writers |
| `BACKFILL_BATCH_JOBS` | `2000` | Records per COPY batch (small repos are batched across repos) |
| `BACKFILL_QUEUE_REPOS` | `256` | Parsed repos allowed to wait for a writer |
| `BACKFILL_MAX_RECORDS_IN_FLIGHT` | `250000` | Records accepted by the sink but not yet committed. Parsed records are JSON several times their CBOR size, so this is the sink's memory bound and the demand gate |
| `BACKFILL_DB_POOL_SIZE` | `writers * 8` | Connections for the backfill's own pool |
| `BACKFILL_SPILL_MB` / `BACKFILL_SPILL_DIR` | `1` / `$TMPDIR` | Archives larger than this stream to disk instead of memory |
| `BACKFILL_MAX_BODY_MB` | `512` | Refuse archives larger than this |
| `BACKFILL_FETCH_TIMEOUT_SECS` | `300` | Whole-request timeout per archive |
| `BACKFILL_REENUMERATE_SECS` | `0` | Re-run enumeration this long after a pass completes; `0` enumerates once and then only drains |
| `BACKFILL_WORKER_THREADS` | CPUs | Tokio threads for the backfill runtime |
| `BACKFILL_SHUTDOWN_GRACE_SECS` | `20` | On stop, how long in-flight fetches and uncommitted writes may finish before they are abandoned to the claimed-row recovery on the next start; keep it under the unit's `TimeoutStopSec` |
| `MIMALLOC_PURGE_DELAY` | (allocator default) | Set to `0` on memory-tight hosts so freed whale-repo allocations return to the OS promptly; the backfill CLI and daemon both use mimalloc |

## Utilities

### backfill

The backfill exposed as a CLI, so each stage can be run and measured on its own. Reads the same `BACKFILL_*` environment as the daemon; the mode defaults to `hybrid` when unset.

```bash
# Which hosts will be fetched directly
./target/release/backfill discover

# Walk listRepos into state (direct hosts first, then hubble); resumable
./target/release/backfill enumerate
./target/release/backfill enumerate --source morel.us-east.host.bsky.network --max-pages 5

# Counts by state and by source
./target/release/backfill status

# Fetch and index everything pending, then exit
DATABASE_URL=... ./target/release/backfill drain

# Fetch + parse one repo, write nothing
./target/release/backfill probe --did did:plc:abc123
./target/release/backfill probe --did did:plc:abc123 --host morel.us-east.host.bsky.network

# Measure the fetch side without a database
BACKFILL_SINK=null ./target/release/backfill drain
```

### direct_index / car_loader

`direct_index` indexes a handful of repos synchronously (bypassing the state store); `car_loader` bulk-loads CAR files from disk. Both share the backfill's CAR parser.

## Queues and state

| Store | Purpose |
|-------|---------|
| `firehose_live` (fjall) | Live records awaiting indexing |
| `label_live` (fjall) | Labels awaiting indexing |
| `backfill_state.sqlite` | Backfill: per-repo `source`, listed `rev`, `indexed_rev`, state, attempts, cooldown, resync priority; per-host enumeration cursor, list state, adaptive concurrency floor, cooldown |
| `repo_sync` (PostgreSQL) | Live sync 1.1 state: per-repo `rev` and `data_cid` (MST root) of the last commit seen on the firehose, plus the relay `host`. Created by `migrations/create_repo_sync.sql` |

Backfill has no record queue: parsed repos go to the COPY writers through a bounded channel, and fetch workers only claim work while that channel has room.

**Cursor state:** Firehose and label cursors are in the PostgreSQL `sub_state` table; backfill enumeration cursors (text, since hubble's are DIDs) are in the state file.

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
- `ingester_label_live_length` - Current label_live queue size
- `ingester_websocket_connections` - Active WebSocket connections
- `ingester_errors_total` - Ingestion errors by type
- `ingester_sync11_commits_total{outcome}` - Live `#commit` frames checked against stored sync state: `applied`, `first_seen`, `stale`, `lax`, `desync`, `no_data`
- `ingester_sync11_sync_events_total{outcome}` - Live `#sync` frames: `resync` or `unchanged`
- `ingester_sync11_resyncs_requested_total{reason}` - Repos handed to backfill for a full resync: `prev_mismatch` or `sync_event`
- `ingester_live_commits_dropped_total{reason}` - Live `#commit` frames dropped whole before enqueue: `stale_replay` (the rev does not advance what the sync 1.1 tracker holds for the repo)
- `ingester_identity_tasks_in_flight` - Identity/account/sync tasks running (bounded by `IDENTITY_EVENT_CONCURRENCY`)
- `ingester_identity_task_timeouts_total{kind}` / `ingester_identity_tasks_shed_total{kind}` - Tasks abandoned at the deadline; events dropped because every permit was busy
- `wintermute_rss_bytes` / `wintermute_rss_peak_bytes` - Resident set size and its high-water mark (Linux; 0 elsewhere)
- `wintermute_fjall_write_buffer_bytes`, `wintermute_fjall_journal_count`, `wintermute_fjall_disk_bytes` - Fjall memtable bytes, open journals, on-disk size
- `indexer_records_processed_total` - Total records processed
- `indexer_records_failed_total` - Failed record indexing
- `indexer_stale_writes_skipped_total` - Skipped stale writes (older rev)
- `indexer_post_events_total` - Posts indexed
- `indexer_like_events_total` - Likes indexed
- `indexer_follow_events_total` - Follows indexed
- `indexer_repost_events_total` - Reposts indexed
- `indexer_block_events_total` - Blocks indexed
- `indexer_profile_events_total` - Profiles indexed
- `backfill_repos_fetched_total{source}` / `backfill_bytes_fetched_total{source}` - Archives fetched, by `hubble` / `bsky` / `pds`
- `backfill_fetch_failures_total{source,class}` - Fetch failures by class (`rate_limited`, `server_5xx`, `server_5xx_app`, `transport`, `terminal`)
- `backfill_records_parsed_total` / `backfill_records_written_total` - Records through the parser and committed by the sink
- `backfill_repos_done_total` / `backfill_repos_terminal_total` - Repos completed, repos written off
- `backfill_repos_by_state{state}` - State-store rows by `pending` / `claimed` / `done` / `terminal`
- `backfill_active_workers`, `backfill_in_flight_fetches`, `backfill_sink_queued_repos`, `backfill_sink_records_in_flight` - Where the pipeline is
- `backfill_gate_waits_total` - Times a fetch worker paused because the writers were full (this is the demand gate working)
- `backfill_host_reductions_total` / `backfill_host_cooldowns_total` - Adaptive concurrency events
- `backfill_write_seconds` - Wall time per COPY batch

## Operations

### Cursor Management

Firehose and label cursors are stored in the PostgreSQL `sub_state` table. On restart, wintermute resumes from the last saved cursor position. Cursors are saved every 20 events.

### Backfill demand gate and host budgets

Backfill fetch workers ask the sink for room before claiming a single repo: the channel into the COPY writers is bounded (`BACKFILL_QUEUE_REPOS`), so nothing is pulled from a PDS or hubble that PostgreSQL is not ready to take. Live events are indexed by their own loop and pools and are never blocked by backfill.

Every source has its own worker and token bucket. Bluesky's mushrooms run at a fixed rate and never throttle down. Any other direct host steps its in-flight ceiling down by a quarter after three consecutive transient errors (429, proxy 5xx, connect/timeout), recovers one unit after four quiet minutes, and is parked (honouring `Retry-After`, capped at 300 s) once it exhausts the floor. A repo its own PDS will not serve is handed to hubble with a fresh attempt budget.

### Sync 1.1 gap detection

Every `#commit` frame from a sync 1.1 host carries `prevData`, the MST root of the commit it follows. The ingester keeps the last applied `(rev, data)` per repo and checks each frame against it, the same inductive proof hubble-sync applies:

| Frame vs stored state | Outcome | Effect |
|-----------------------|---------|--------|
| Nothing stored | `first_seen` | Recorded. History comes from backfill enumeration, not from resyncing every unknown repo |
| `rev` not newer than stored | `stale` | A replay; state is not advanced (indexing is idempotent and unaffected) |
| `prevData` present and not equal to the stored root | `desync` | A resync is requested; state resets to this commit so the chain continues |
| No commit block in the frame (`tooBig`) | `no_data` | Stored state is forgotten rather than left to trip the next frame |
| No `prevData` (pre-sync-1.1 host) | `lax` | Recorded but unproven; never a resync on its own |
| `prevData` equals the stored root | `applied` | The proof holds |

A `#sync` frame means the repo's MST was rewritten upstream: unless its commit matches what is stored exactly, a resync is requested and the new root recorded. An `#account` frame taking a repo out of service (`deleted`, `takendown`, `deactivated`, `suspended`) writes the repo off in the backfill state store so no fetch is spent on it, and drops its sync state.

A resync is a row in `backfill_state.sqlite` set back to `pending` with `indexed_rev` cleared (`last_error` says `resync: prev_mismatch` or `resync: sync_event`); the backfill runner's next tick fetches the whole repo through whichever source owns it. Resync requests carry `priority = 1` and are claimed ahead of every enumerated repo for their source, so a repo the firehose has proved out of sync is fetched next rather than queued behind millions of pending rows. With `BACKFILL_MODE=off` requests are still recorded and are fetched once backfill is enabled; the first such request is logged at `info`.

Cost on the live path: the check runs in a single tracker task behind a bounded channel, with a bounded in-memory cache (two generations of 150k repos) in front of `repo_sync`. Postgres is read only on cache miss, one query per drained batch, and written once a second as one `unnest` upsert per repo touched in that second.

Stale replays are dropped before they reach the queue, not just left unrecorded: with several `RELAY_HOSTS` a relay running hours behind re-delivers commits the others already carried, so the firehose loop checks every `#commit` against the tracker's cache (shared with it as a synchronous read-only view) and, when the rev does not advance what is held, enqueues none of its creates, updates or deletes and counts it in `ingester_live_commits_dropped_total{reason="stale_replay"}`; the indexer's per-row rev gate cannot do this for a record deleted in between, because there is no row left to gate on. The gate is only as warm as the cache: a repo not seen since the process started passes its first commit through (the tracker then loads its `repo_sync` row and catches the next replay), as does a replay arriving within the tracker's channel lag of the original.

### Handle Resolution

Handles are resolved asynchronously after initial indexing. Actors with NULL handles are prioritized and re-checked every hour. Valid handles are re-verified every 24 hours.

### Graceful Shutdown

On SIGTERM or SIGINT, wintermute:
1. Stops accepting new events
2. Drains all in-flight work; the backfill gives in-flight fetches and uncommitted writes `BACKFILL_SHUTDOWN_GRACE_SECS` (default 20 s) to land, then aborts what is left -- those repos stay claimed and are re-fetched on the next start
3. Saves cursor positions
4. Exits cleanly

### Recovery

Fjall queues and the backfill state file are durable and survive crashes. On restart, wintermute:
1. Resumes firehose from saved cursor
2. Returns any backfill repos claimed by the dead process to pending and resumes enumeration from the persisted per-source cursors
3. Reprocesses any in-flight records that weren't acknowledged

## Requirements

### System Requirements

- **Memory**: 8GB minimum for the live daemon with the default Fjall settings; the backfill's footprint is governed by `BACKFILL_MAX_INFLIGHT` / `BACKFILL_MAX_RECORDS_IN_FLIGHT` (see the memory settings above)
- **Storage**: ~7 GB for the backfill state file at full-network scale, plus spill space for large archives
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

## Running the database-backed tests

`indexer::tests` and `ingester::tests` write to a real PostgreSQL that carries the
appview dataplane schema. They read `DATABASE_URL` and default to
`postgresql://postgres:postgres@localhost:5432/bsky_test`. A snapshot of that schema
lives in [`tests/schema/appview.sql`](tests/schema/appview.sql); CI applies it to a
stock `postgres:17` container before `cargo test`, and you can do the same locally:

```bash
docker run -d --name wintermute-pg -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=bsky_test -p 5432:5432 postgres:17
until docker exec wintermute-pg pg_isready -U postgres -d bsky_test; do sleep 1; done
docker exec -i wintermute-pg psql -U postgres -d bsky_test -v ON_ERROR_STOP=1 \
  < rsky-wintermute/tests/schema/appview.sql

DATABASE_URL=postgresql://postgres:postgres@localhost:5432/bsky_test \
  cargo test -p rsky-wintermute
```

The snapshot creates every object in `public` (the `bsky.` qualifiers from the dump
are stripped) so the tests' default URL works without a `search_path` option.
Production runs the same tables in schema `bsky` with `search_path=bsky` set on the
connection string; wintermute's queries are unqualified, so both layouts behave the
same. A few tests (`test_live_label_stream`, the hubble `getRepo` fetch) hit the
network and are `#[ignore]`d.

### Regenerating the schema snapshot

The file is a cleaned `pg_dump` of a database migrated with the
[blacksky-algorithms/atproto](https://github.com/blacksky-algorithms/atproto) fork's
`packages/bsky` kysely migrations (currently `_20260816T120000000Z`). When those
migrations change, redump from any database at the new migration head:

```bash
pg_dump --schema-only --no-owner --no-privileges --no-comments -n bsky appview_db \
  > appview-bsky.sql
```

Then, to produce `tests/schema/appview.sql`: drop the `\restrict`/`\unrestrict`
lines, the `SET ...`/`SELECT pg_catalog.set_config(...)` preamble and
`CREATE SCHEMA bsky;`; strip the `bsky.` schema qualifier from every identifier
(including the `nextval('bsky....')` defaults); prepend
`CREATE EXTENSION IF NOT EXISTS pg_trgm;` for the trigram indexes; keep the
provenance header up to date; and do not include any rows (in particular the
`kysely_migration` rows). Verify with the docker commands above before committing.

## License

rsky-wintermute is released under the [Apache License 2.0](../LICENSE).
