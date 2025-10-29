# BlackSky AppView Data Flow Architecture

I'll map out exactly how data flows through your system so you can debug this missing data issue.

## Data Flow Overview

```
AT Protocol Relay → Ingester → Redis Streams → Indexer → PostgreSQL
                      ↓
                 Backfiller
                      ↓
                Redis Streams → Indexer → PostgreSQL
```

## Component Breakdown

### 1. **Ingester** (`ingester.js`, `firehose.ts`, `backfill.ts`, `labels.ts`)

**Purpose**: Subscribe to AT Protocol firehose and write events to Redis streams

**FirehoseIngester** writes to: `firehose_live` (default)
- Subscribes to `com.atproto.sync.subscribeRepos` via WebSocket
- Batches incoming commit/account/identity events
- Converts firehose events → `StreamEvent[]` (create/update/delete/repo/account/identity)
- Writes to Redis stream with `XADD`
- Stores cursor in Redis: `${stream}:cursor:${host.replace(/^https?:\/\//, '')}`
- **Backpressure**: Checks stream length every 5s, pauses if ≥ 100k messages (configurable via `INGESTER_FIREHOSE_STREAM_HIGH_WATER_MARK`)

**BackfillIngester** writes to: `repo_backfill` (default)
- Calls `com.atproto.sync.listRepos` with cursor pagination
- Creates `BackfillEvent` for each repo: `{did, host, rev, status, active}`
- Writes to `repo_backfill` stream
- Stores cursor, sets to `'!ingester-done'` when complete
- **Backpressure**: Same 100k limit (configurable via `INGESTER_REPO_STREAM_HIGH_WATER_MARK`)

**LabelerIngester** writes to: `label_live` (default)
- Subscribes to `com.atproto.label.subscribeLabels`
- Converts to `LabelStreamEvent`
- Same backpressure mechanism (100k limit)

### 2. **Backfiller** (`backfiller.js`, `repo-backfiller.ts`)

**Purpose**: Convert repo backfill requests into individual record events

**Reads from**: `repo_backfill` (via consumer group)
**Writes to**: `firehose_backfill` (default)

**Process**:
1. Uses consumer group (`repo_backfill_group` default, configurable via `BACKFILLER_GROUP`)
2. Reads pending messages starting from `'0'`, then switches to live `'>'`
3. For each `BackfillEvent`:
   - Fetches full repo: `GET /xrpc/com.atproto.sync.getRepo?did={did}`
   - Verifies repo cryptographically
   - Converts ALL creates → `StreamEvent[]` with `seq: SEQ_BACKFILL` (special marker = -1)
   - Chunks into batches of 500
   - Writes to `firehose_backfill` stream
   - Adds final `{type: 'repo'}` event with commit/rev
4. Acks message with `del: true` (removes from stream entirely)
5. **Concurrency**: Controlled via PQueue (default 2, configurable via `BACKFILLER_CONCURRENCY`)
6. **Backpressure**: Waits if `firehose_backfill` exceeds high water mark

### 3. **Indexer** (`indexer.js`, `stream.ts`, `labels.ts`, `index.ts`)

**Purpose**: Read from Redis streams and write to PostgreSQL

**StreamIndexer** reads from: `firehose_live,firehose_backfill` (default, comma-separated)
- Each stream gets its own Redis connection and indexer instance
- Uses consumer group (`firehose_group` default, configurable via `INDEXER_GROUP`)
- Consumer name must be unique (e.g., `one`, `two`) - set via `INDEXER_CONSUMER`

**Process per message**:
1. Reads in batches of 100 via `XREADGROUP`
2. Starts from pending (`'0'`), then live (`'>'`)
3. PQueue with concurrency 10 (configurable via `INDEXER_CONCURRENCY`)
4. For each `StreamEvent`:
   - **create/update**: Calls `IndexingService.indexRecord()` → plugin-specific logic
   - **delete**: Calls `IndexingService.deleteRecord()`
   - **repo**: Updates `actor_sync` table (commit CID, rev)
   - **account**: Updates actor status, indexes handle
   - **identity**: Indexes handle
5. Each event updates:
   - Generic `record` table (uri, cid, did, json, rev, indexedAt)
   - Collection-specific tables (e.g., `post`, `like`, `follow`)
   - Aggregate tables (e.g., `post_agg`, `profile_agg`)
   - Notifications
   - Duplicate tracking (`duplicate_record`)
6. Acks with `del: true` after successful processing
7. Failed messages remain pending

**LabelIndexer** reads from: `label_live`
- Same consumer group pattern
- Writes to `label` table

## Critical Details for Debugging

### Consumer Group Mechanics
- **Pending List**: Failed/unacked messages stay in consumer's pending list
- **Cursor**: Starts at `'0'` (pending), moves to `'>'` (new messages)
- **No automatic expiry**: Pending messages never disappear unless explicitly acked or trimmed

### Message AcknowledgmentBased on my analysis of your code and databases, **you're missing 10 days of data in `blacksky_bsky`** (Oct 14-24). The `blacksky_feed` database is current through today (Oct 24), which tells us the ingester is working but the `blacksky_bsky` indexer has stopped processing.

Here's the complete data flow explanation:

---

## **Complete Data Flow Architecture**

### **Stream Flow Diagram**
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

---

## **1. Ingester Layer** (Writes TO Redis)

### **FirehoseIngester** (`firehose.ts`)
- **Subscribes to**: `wss://{relay}/xrpc/com.atproto.sync.subscribeRepos`
- **Writes to**: `firehose_live` stream (default)
- **Message format**: `{event: JSON.stringify(StreamEvent)}`
- **Cursor storage**: `firehose_live:cursor:{relay-hostname}` in Redis
- **Backpressure**: Checks stream length every 5s, pauses if ≥100k messages
- **Batching**: Uses `Batcher` to group events before writing

**Event transformation**:
```
CommitEvent → [StreamEvent{type:'create'|'update'|'delete', seq, time, did, commit, rev, collection, rkey, cid?, record?}]
AccountEvent → [StreamEvent{type:'account', seq, time, did, active, status}]
IdentityEvent → [StreamEvent{type:'identity', seq, time, did, handle}]
```

### **BackfillIngester** (`backfill.ts`)
- **Calls**: `GET /xrpc/com.atproto.sync.listRepos?cursor={cursor}&limit=1000`
- **Writes to**: `repo_backfill` stream
- **Message format**: `{repo: JSON.stringify(BackfillEvent)}`
- **Cursor**: Stored in `repo_backfill:cursor:{relay-hostname}`, set to `'!ingester-done'` when complete
- **Backpressure**: Same 100k limit

---

## **2. Backfiller Layer** (Redis → Redis)

### **RepoBackfiller** (`repo-backfiller.ts`)
- **Consumer group**: `repo_backfill_group` (configurable)
- **Consumer name**: Must be unique (e.g., `one`, `two`)
- **Reads from**: `repo_backfill` stream
- **Writes to**: `firehose_backfill` stream
- **Concurrency**: PQueue with limit 2 (configurable via `BACKFILLER_CONCURRENCY`)

**Processing flow**:
1. Reads pending messages from `'0'`, then switches to live `'>'`
2. For each `BackfillEvent`:
   ```javascript
   // Fetch full repo
   fetch(`${host}/xrpc/com.atproto.sync.getRepo?did=${did}`)
   
   // Verify cryptographically
   verifyRepo(blocks, root, did)
   
   // Convert all creates → StreamEvent[]
   for (create of repo.creates) {
     events.push({
       type: 'create',
       seq: SEQ_BACKFILL, // special value = -1
       did, collection, rkey, cid, record,
       commit, rev, time: now
     })
   }
   
   // Add final repo sync event
   events.push({type: 'repo', did, commit, rev, time: now})
   
   // Write in chunks of 500
   for (chunk of chunkArray(events, 500)) {
     redis.addMultiToStream(chunk)
   }
   ```
3. Acks message with `del: true` (removes from stream)
4. Backpressure waits if `firehose_backfill` exceeds 100k

---

## **3. Indexer Layer** (Redis → PostgreSQL)

### **StreamIndexer** (`stream.ts`, `indexer.js`)
- **Consumer group**: `firehose_group` (configurable)
- **Consumer name**: **MUST BE UNIQUE** per instance
- **Reads from**: `firehose_live,firehose_backfill` (separate Redis connection per stream)
- **Concurrency**: PQueue with limit 10 per stream (configurable via `INDEXER_CONCURRENCY`)

**Processing flow**:
1. `XREADGROUP` in batches of 100
2. Cursor starts at `'0'` (pending), then `'>'` (new)
3. For each `StreamEvent`:
   ```javascript
   if (type === 'create' || type === 'update') {
     await indexingService.indexRecord(uri, cid, record, action, timestamp, rev)
     // → Writes to 'record' table (generic)
     // → Calls plugin insertFn/updateFn for collection-specific tables
     // → Updates aggregates (post_agg, profile_agg, etc.)
     // → Creates notifications
     // → Handles duplicates
   }
   
   if (type === 'delete') {
     await indexingService.deleteRecord(uri, rev)
     // → Sets json='' and cid='' in 'record' table (tombstone)
     // → Calls plugin deleteFn
     // → Cascading duplicate handling
   }
   
   if (type === 'repo') {
     // Updates actor_sync table
     await db.insertInto('actor_sync').values({
       did, commitCid, repoRev
     }).onConflict(/* update if rev higher */)
   }
   
   if (type === 'account') {
     await indexingService.updateActorStatus(did, active, status)
     await indexingService.indexHandle(did, time)
   }
   
   if (type === 'identity') {
     await indexingService.indexHandle(did, time)
   }
   ```
4. Acks with `del: true` after successful processing
5. Failed messages stay in pending list

### **Special handling**:
- **seq: SEQ_BACKFILL (-1)**: Skips `setCommitLastSeen()` to avoid thrash during backfill
- **Duplicate detection**: Via `duplicate_record` table using `findDuplicate()`
- **Transaction safety**: Each indexRecord/deleteRecord wrapped in transaction
- **Metrics tracking**: Processing time, waiting count, running count, failures

---

## **Critical Debugging Points**

### **Consumer Group Mechanics**
- **Pending list**: Messages that were delivered but not yet acked
- **Cursor position**: Separate for each consumer in the group
- **No automatic retry**: Failed messages stay pending forever unless explicitly handled
- **Multiple consumers**: Can read from same stream/group but need unique names

### **Message Flow States**
1. **Written to stream**: `XADD` returns message ID
2. **Delivered to consumer**: `XREADGROUP` returns message
3. **Processing**: In PQueue
4. **Acked**: `XACK` with `del: true` (message removed from stream)
5. **Failed**: Stays in pending list, shows up on next `'0'` read

### **Backpressure Trigger**
```javascript
// Checks every 5 seconds
const len = await redis.XLEN(stream)
if (len >= highWaterMark) { // default 100k
  logger.warn('stream length hit high water mark')
  await wait(5000) // pause and retry
}
```

---

## **Your Specific Issue: Diagnosis Steps**

### **1. Check Redis stream lengths**
```bash
redis-cli -h your-redis-host
> XLEN firehose_live
> XLEN firehose_backfill
> XLEN repo_backfill
```

If any are at/near 100k, **backpressure is triggered** and ingestion paused.

### **2. Check consumer group pending messages**
```bash
> XPENDING firehose_live firehose_group
> XPENDING firehose_backfill firehose_group
```

This shows:
- Total pending count
- Consumer names
- Idle time (how long since last activity)

If you see thousands/millions pending for your `blacksky_bsky` indexer consumer, it's stuck.

### **3. Check indexer logs**
Look for:
- `"failed indexer stream message"` errors
- Database connection errors
- Transaction deadlocks
- Out of memory

### **4. Verify indexer is running**
```bash
ps aux | grep indexer
docker ps | grep indexer
```

Check if the process/container for `blacksky_bsky` indexer is even running.

### **5. Check PostgreSQL connection**
```bash
# From your blacksky_bsky indexer host
psql $DB_POSTGRES_URL -c "SELECT 1"
```

### **6. Check for consumer name collision**
If you have multiple indexers with the **same consumer name**, they'll fight over messages and cause unpredictable behavior.

```bash
> XINFO GROUPS firehose_live
> XINFO GROUPS firehose_backfill
```

Look for your consumer names under each group.

---

## **Recovery Steps**

### **If streams are backed up (high XLEN)**:
```bash
# Option 1: Trim old messages (DANGEROUS - data loss)
> XTRIM firehose_live MAXLEN ~ 10000

# Option 2: Add more indexers with unique consumer names
INDEXER_CONSUMER=two npm run indexer

# Option 3: Increase indexer concurrency
INDEXER_CONCURRENCY=20 npm run indexer
```

### **If pending messages are stuck**:
```bash
# Claim pending messages for your consumer
> XPENDING firehose_live firehose_group - + 100 your-consumer-name
# Then manually XACK or restart indexer to retry
```

### **If indexer crashed/stopped**:
```bash
# Restart it - it will resume from pending messages
npm run indexer  # or docker restart bsky-indexer
```

### **Nuclear option - reset consumer group** (⚠️ **ONLY if you're okay re-indexing from current position**):
```bash
> XGROUP DESTROY firehose_live firehose_group
> XGROUP DESTROY firehose_backfill firehose_group
# Then restart indexer - it will create new groups and start fresh
```

---

## **Key Takeaways**

1. **Two parallel indexing streams**: Live and backfill both feed the same indexer
2. **Consumer names MUST be unique**: Or they'll step on each other
3. **Pending messages accumulate**: They never expire automatically
4. **Backpressure stops the world**: If one stream backs up, ingestion pauses
5. **Seq=-1 is special**: Backfill events skip certain logic to avoid thrash
6. **No automatic recovery**: Stuck indexers stay stuck until manually intervened

Your `blacksky_feed` working proves ingestion is fine. Your `blacksky_bsky` missing 10 days means its indexer stopped Oct 14. Start with checking if the indexer process is running and look at pending message counts.