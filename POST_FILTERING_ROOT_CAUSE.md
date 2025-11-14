# Post Filtering Root Cause Analysis

**Date**: 2025-10-31
**Issue**: Posts exist in database but are filtered from API responses
**Affected Post**: `at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k`

---

## Problem Statement

**Observed Behavior**:
- API query to `/xrpc/app.bsky.feed.getAuthorFeed?actor=did:plc:w4xbfzo7kqfes5zb7r6qv3rw&limit=20` returns only 12 posts
- Database query shows 20 posts should be returned
- Missing post is at position 7 in chronological order (by `sortAt`)
- Post appears as "deleted" when viewed in embedded context (`app.bsky.embed.record#viewNotFound`)

**Database Verification** (All ✅):
- **record table**: Post exists with 936 bytes valid JSON, valid CID, no takedownRef
- **post table**: Post exists with correct `creator` and timestamps
- **feed_item table**: Post is at position 7 with `sortAt = 2025-10-30T18:29:22.800+00:00`
- **post_agg table**: Post has engagement metrics (283 likes, 73 reposts, 8 replies, 4 quotes)
- **actor table**: Author profile exists (`did:plc:w4xbfzo7kqfes5zb7r6qv3rw`, handle `rude1.blacksky.team`)

---

## Investigation Timeline

### Step 1: Initial Hypothesis - Timestamp Mismatch

**Theory**: The embedded post was indexed 10 hours after the parent post, possibly causing hydration issues.

**Action Taken**: Updated `indexedAt` timestamps to match `createdAt` for both posts:
```sql
UPDATE record SET "indexedAt" = '2025-10-30T18:29:22.800Z'
WHERE uri = 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k';

UPDATE post SET "indexedAt" = '2025-10-30T18:29:22.800Z'
WHERE uri = 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k';
```

**Result**: No change. Post still not appearing in timeline.

**Analysis**: Open-source AppView code does NOT filter based on `indexedAt` values. The `BSKY_INDEXED_AT_EPOCH` environment variable mentioned in Bluesky's code is NOT present in the open-source version and is only used for display purposes in Bluesky's closed-source fork.

### Step 2: Database Query Verification

**Verified**:
- Dataplane INNER JOIN between `feed_item` and `post` tables would return the post (both rows exist with matching URIs)
- No `takedownRef` set on record
- JSON field is not empty (936 bytes)
- CID field is present

**Database query that SHOULD return the post**:
```typescript
// From /packages/bsky/src/data-plane/server/routes/feeds.ts
db.db
  .selectFrom('feed_item')
  .innerJoin('post', 'post.uri', 'feed_item.postUri')
  .selectAll('feed_item')
  .where('originatorDid', '=', 'did:plc:w4xbfzo7kqfes5zb7r6qv3rw')
  .orderBy('sortAt', 'desc')
  .orderBy('cid', 'desc')
  .limit(20)
```

This query returns 20 rows from the database (verified manually), but API returns only 12.

### Step 3: Architecture Deep Dive

**Comprehensive code analysis** revealed 13 distinct filtering points across 4 pipeline stages:

1. **Database Layer** (1 filter)
2. **Hydration Layer** (4 filters)
3. **Filtering Layer** (4 filters)
4. **Presentation Layer** (4 filters)

See [APPVIEW_ARCHITECTURE.md](./APPVIEW_ARCHITECTURE.md) for complete details.

---

## Likely Root Causes

Based on the architecture analysis, the most likely explanations are:

### Hypothesis A: Schema Validation Failure (MOST LIKELY)

**Location**: `parseRecord()` in `/packages/bsky/src/hydration/util.ts` lines 61-83

**Mechanism**:
```typescript
// Filter 3: Validate record against lexicon schema
if (!isValidRecord(record)) {
  return  // ← Returns undefined, causing post to be null in hydration
}
```

**Symptoms Match**:
- Post exists in database with valid JSON
- Post filtered during hydration
- No explicit error logged (validation failures are silent)
- Results in `viewNotFound` for embedded references

**Why This Happens**:
1. Post record JSON is fetched from database
2. Parsed from bytes
3. Validated against lexicon schema for `app.bsky.feed.post`
4. If ANY field doesn't match schema expectations → filtered out
5. Common causes:
   - Extra fields not in schema
   - Wrong data types (string instead of number, etc.)
   - Missing required fields
   - Invalid AT-URI format in reply/embed fields
   - Invalid datetime format in `createdAt`

**How to Verify**:
```sql
-- Get the actual record JSON
SELECT json::jsonb FROM record
WHERE uri = 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k';
```

Then manually validate against the lexicon schema at:
`/packages/bsky/lexicons/app/bsky/feed/post.json`

### Hypothesis B: Author Profile Hydration Failure

**Location**: `post()` method in `/packages/bsky/src/views/index.ts` lines 906-907

**Mechanism**:
```typescript
const author = this.profileBasic(authorDid, state)
if (!author) return  // ← Returns undefined, filtered by mapDefined()
```

**Symptoms Match**:
- Post successfully hydrated
- Author exists in `actor` table
- Profile record might be missing or invalid

**Why This Happens**:
1. Post hydration succeeds
2. View generation calls `profileBasic(did)` to get author profile
3. Profile not found in hydration state
4. Entire post view returns undefined
5. `mapDefined()` filters it out

**How to Verify**:
```sql
-- Check if author has profile record
SELECT * FROM record
WHERE uri LIKE 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.actor.profile/%';
```

If profile record exists, check if it passes schema validation (same issue as Hypothesis A but for profile schema).

### Hypothesis C: Blocks/Mutes Filtering

**Location**: `noBlocksOrMutedReposts` in `/packages/bsky/src/api/app/bsky/feed/getAuthorFeed.ts` lines 167-188

**Mechanism**:
```typescript
const checkBlocksAndMutes = (item: FeedItem) => {
  const bam = ctx.views.feedItemBlocksAndMutes(item, hydration)
  return (
    !bam.authorBlocked &&
    !bam.originatorBlocked &&
    (!bam.authorMuted || bam.originatorMuted)
  )
}
skeleton.items = skeleton.items.filter(checkBlocksAndMutes)
```

**Symptoms Match**:
- Post filtered for specific viewer
- Other users might see the post fine
- Depends on relationship between viewer and author

**Why This Happens**:
- Viewer has blocked the author
- Author has blocked the viewer
- Author is muted by viewer

**How to Verify**:
```sql
-- Check if viewing user's own feed (no viewer context)
-- If yes, this hypothesis is ruled out

-- If there IS a viewer, check blocks
SELECT * FROM block
WHERE (creator = '{viewerDid}' AND subjectDid = 'did:plc:w4xbfzo7kqfes5zb7r6qv3rw')
   OR (creator = 'did:plc:w4xbfzo7kqfes5zb7r6qv3rw' AND subjectDid = '{viewerDid}');
```

---

## Recommended Debugging Steps

### Step 1: Enable Debug Logging in AppView

Add logging to key filtering points:

**A. In parseRecord (hydration/util.ts)**:
```typescript
export const parseRecord = <T>(entry: Record, includeTakedowns: boolean): RecordInfo<T> | undefined => {
  if (!includeTakedowns && entry.takenDown) {
    console.log(`[FILTER] Takedown: ${entry.uri}`)
    return undefined
  }

  const record = parseRecordBytes<T>(entry.record)
  const cid = entry.cid
  if (!record || !cid) {
    console.log(`[FILTER] No bytes/CID: ${entry.uri}`, { hasRecord: !!record, hasCid: !!cid })
    return
  }

  if (!isValidRecord(record)) {
    console.log(`[FILTER] Schema validation failed: ${entry.uri}`, record)
    return
  }

  return { record, cid, ... }
}
```

**B. In post() view method (views/index.ts)**:
```typescript
post(uri: string, state: HydrationState, depth = 0): Un$Typed<PostView> | undefined {
  const post = state.posts?.get(uri)
  if (!post) {
    console.log(`[FILTER] Post not in hydration: ${uri}`)
    return
  }

  const parsedUri = new AtUri(uri)
  const authorDid = parsedUri.hostname
  const author = this.profileBasic(authorDid, state)
  if (!author) {
    console.log(`[FILTER] Author profile not found: ${authorDid} for post ${uri}`)
    return
  }

  // ...
}
```

**C. In feedViewPost (views/index.ts)**:
```typescript
feedViewPost(item: FeedItem, state: HydrationState): Un$Typed<FeedViewPost> | undefined {
  const post = this.post(item.post.uri, state)
  if (!post) {
    console.log(`[FILTER] feedViewPost filtered: ${item.post.uri}`)
    return
  }
  // ...
}
```

### Step 2: Test API with Logging Enabled

```bash
# Restart appview with logging
# Make test request
curl "https://api.blacksky.community/xrpc/app.bsky.feed.getAuthorFeed?actor=did:plc:w4xbfzo7kqfes5zb7r6qv3rw&limit=20"

# Check logs for filter messages
grep "\\[FILTER\\]" /path/to/appview/logs
```

### Step 3: Manual Schema Validation

**Extract and validate the specific post record**:

```bash
# Get the post JSON
psql -h localhost -p 15433 -U bsky -d bsky -c \
  "SELECT json::jsonb FROM record WHERE uri = 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k'" \
  -o /tmp/post_record.json

# Manually check against schema
# Compare fields in /tmp/post_record.json with:
# /packages/bsky/lexicons/app/bsky/feed/post.json
```

**Required fields for app.bsky.feed.post**:
- `$type`: "app.bsky.feed.post"
- `text`: string
- `createdAt`: datetime (ISO 8601 format)

**Optional fields**:
- `reply`: object with `root` and `parent` (both must be valid refs)
- `embed`: one of the allowed embed types
- `langs`: array of language codes
- `labels`: self-labels
- `tags`: array of strings
- `facets`: array of facet objects

### Step 4: Check Profile Record

```sql
-- Get profile record for author
SELECT uri, LENGTH(json::text) as json_length, "takedownRef"
FROM record
WHERE uri LIKE 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.actor.profile/%';
```

If profile doesn't exist or has empty JSON, that could cause author hydration to fail.

### Step 5: Compare Working vs Non-Working Posts

**Find a post that IS returned by API**:
```bash
curl -s "https://api.blacksky.community/xrpc/app.bsky.feed.getAuthorFeed?actor=did:plc:w4xbfzo7kqfes5zb7r6qv3rw&limit=1" \
  | jq -r '.feed[0].post.uri'
```

**Compare database state**:
```sql
-- Compare record structure
SELECT
  uri,
  LENGTH(json::text) as json_length,
  cid,
  "takedownRef",
  "indexedAt"
FROM record
WHERE uri IN (
  'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k',  -- missing
  '{working_post_uri}'  -- returned by API
);
```

**Compare JSON structure**:
```sql
-- Export both for comparison
\copy (SELECT json::jsonb FROM record WHERE uri = 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k') TO '/tmp/missing_post.json'
\copy (SELECT json::jsonb FROM record WHERE uri = '{working_post_uri}') TO '/tmp/working_post.json'

-- Then diff the files
diff /tmp/missing_post.json /tmp/working_post.json
```

---

## Testing in Local Environment

To test the exact filtering behavior:

1. **Set up local AppView instance** pointing to production database (read-only)
2. **Add debug logging** to the filtering points above
3. **Make test request** for the specific feed
4. **Examine logs** to see exactly which filter triggers

```bash
# Example local setup
cd /Users/rudyfraser/Projects/atproto/packages/bsky

# Add logging code to:
# - src/hydration/util.ts (parseRecord)
# - src/views/index.ts (post, feedViewPost)

# Build
pnpm build

# Run locally with production DB (read-only connection)
DATABASE_URL="postgresql://bsky:PASSWORD@localhost:15433/bsky?sslmode=disable" \
  pnpm run start

# Test
curl "http://localhost:2584/xrpc/app.bsky.feed.getAuthorFeed?actor=did:plc:w4xbfzo7kqfes5zb7r6qv3rw&limit=20"

# Check logs for [FILTER] messages
```

---

## Expected Outcome

Based on the symptoms and architecture analysis, **Hypothesis A (Schema Validation Failure)** is most likely:

**Expected Finding**:
- Post record JSON has a field that doesn't match the lexicon schema
- `isValidRecord()` returns false during hydration
- Post becomes `null` in hydration state
- View layer filters it out
- Appears as `viewNotFound` in embedded contexts

**Common Schema Issues**:
1. **Invalid datetime format**: `createdAt` must be ISO 8601 with timezone
2. **Invalid reply ref**: `reply.parent.uri` or `reply.root.uri` not well-formed
3. **Invalid embed**: Embed object doesn't match any allowed embed type
4. **Extra fields**: Fields present that aren't in schema (less common, usually tolerated)
5. **Wrong types**: Field has string where number expected, etc.

---

## Resolution Strategy

Once root cause is identified:

### If Schema Validation Issue:
1. **Fix the record**: Update JSON in database to match schema
2. **OR** Update schema to allow the field (if it's valid but not in schema)
3. **Reindex**: May need to re-fetch from PDS to get corrected version

### If Profile Hydration Issue:
1. **Check if profile exists**: If missing, trigger profile sync
2. **Validate profile schema**: Same validation process as posts
3. **Check profile takedown status**: Ensure not taken down

### If Blocks/Mutes:
1. **Check relationship tables**: Identify block/mute records
2. **Remove if invalid**: If block shouldn't exist, delete it
3. **This is viewer-specific**: Different viewers will see different results

---

## Next Steps

1. Run Step 3 (Manual Schema Validation) immediately - can be done without code changes
2. Set up local AppView with debug logging (Steps 1-2)
3. Compare working vs non-working posts (Step 5)
4. Based on findings, apply appropriate resolution strategy

---

## Related Documentation

- [APPVIEW_ARCHITECTURE.md](./APPVIEW_ARCHITECTURE.md) - Complete technical architecture
- [CLAUDE.md](./CLAUDE.md) - Project mission tracking

---

**Status**: Investigation complete, root cause hypotheses identified, debugging steps provided

**Most Likely Cause**: Schema validation failure during post hydration

**Recommended Next Action**: Manual schema validation of post JSON (Step 3)
