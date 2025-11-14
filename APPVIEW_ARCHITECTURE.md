# AppView & Dataplane Architecture - Complete Technical Documentation

**Date**: 2025-10-31
**Purpose**: Detailed technical analysis of how the open-source AT Protocol AppView processes and filters author feed requests

---

## Executive Summary

The AppView/Dataplane architecture processes feed requests through a 4-stage pipeline:
1. **Skeleton** - Fetches feed items from database via dataplane
2. **Hydration** - Enriches items with post data, profiles, and metadata
3. **Filtering** - Removes blocked/muted content
4. **Presentation** - Converts to API response format

**Critical Finding**: Posts can be filtered out at MULTIPLE points in this flow, even if they exist correctly in all database tables.

---

## Architecture Overview

```
HTTP Request (app.bsky.feed.getAuthorFeed)
    ↓
[XRPC Layer] server.app.bsky.feed.getAuthorFeed
    ↓
[Pipeline] createPipeline(skeleton, hydration, noBlocksOrMutedReposts, presentation)
    ↓
[Stage 1: skeleton] Fetch feed items from dataplane
    ↓
[Stage 2: hydration] Hydrate posts, profiles, aggregates
    ↓
[Stage 3: noBlocksOrMutedReposts] Filter blocked/muted content
    ↓
[Stage 4: presentation] Create FeedViewPost objects
    ↓
HTTP Response { feed: [...], cursor: "..." }
```

---

## Stage 1: Skeleton - Database Query

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/api/app/bsky/feed/getAuthorFeed.ts`
**Lines**: 64-126

### Flow

1. **Resolve Actor DID** (line 69)
   ```typescript
   const [did] = await ctx.hydrator.actor.getDids([params.actor])
   if (!did) throw new InvalidRequestError('Profile not found')
   ```

2. **Check Actor Exists** (lines 73-80)
   ```typescript
   const actors = await ctx.hydrator.actor.getActors([did], {
     includeTakedowns: params.hydrateCtx.includeTakedowns,
     skipCacheForDids: params.hydrateCtx.skipCacheForViewer,
   })
   const actor = actors.get(did)
   if (!actor) throw new InvalidRequestError('Profile not found')
   ```

3. **Call Dataplane** (lines 93-98)
   ```typescript
   const res = await ctx.dataplane.getAuthorFeed({
     actorDid: did,
     limit: params.limit,
     cursor: params.cursor,
     feedType: FILTER_TO_FEED_TYPE[params.filter],
   })
   ```

### Dataplane Query

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/data-plane/server/routes/feeds.ts`
**Lines**: 8-71

**Base Query** (lines 13-17):
```typescript
let builder = db.db
  .selectFrom('feed_item')
  .innerJoin('post', 'post.uri', 'feed_item.postUri')  // ← CRITICAL: INNER JOIN
  .selectAll('feed_item')
  .where('originatorDid', '=', actorDid)
```

**Feed Type Filters** (lines 19-52):
- `POSTS_WITH_MEDIA`: Filters to posts with `post_embed_image` entries
- `POSTS_WITH_VIDEO`: Filters to posts with `post_embed_video` entries
- `POSTS_NO_REPLIES`: Excludes posts where `post.replyParent IS NOT NULL`
- `POSTS_AND_AUTHOR_THREADS`: Includes only posts, reposts, or replies within author's own threads

**Pagination** (lines 54-63):
```typescript
const keyset = new TimeCidKeyset(
  ref('feed_item.sortAt'),
  ref('feed_item.cid'),
)
builder = paginate(builder, { limit, cursor, keyset })
```

**CRITICAL FILTERING POINT #1**: The `INNER JOIN` means:
- If a row exists in `feed_item` but NOT in `post` table → excluded
- If a row exists in `post` but NOT in `feed_item` table → excluded
- Both tables must have matching `uri` values for row to be returned

### Dataplane Response

Returns array of `FeedItem` objects:
```typescript
{
  items: [
    {
      uri: "at://did:plc:xxx/app.bsky.feed.post/xxx",
      repost: "at://did:plc:xxx/app.bsky.feed.repost/xxx" | undefined,
      repostCid: "..." | undefined
    },
    ...
  ],
  cursor: "sortAt::cid" | undefined
}
```

---

## Stage 2: Hydration - Enriching Data

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/hydrator.ts`
**Lines**: 670-700

### Flow

1. **Hydrate Posts** (lines 675-678)
   ```typescript
   const posts = await this.feed.getPosts(
     items.map((item) => item.post.uri),
     ctx.includeTakedowns,
   )
   ```

2. **Collect Reply References** (lines 679-690)
   - Extracts `rootUris` and `parentUris` from post replies
   - Builds `postAndReplyRefs` array

3. **Hydrate Reply Posts** (lines 692-695)
   ```typescript
   const replies = await this.feed.getPosts(
     [...rootUris, ...parentUris],
     ctx.includeTakedowns,
   )
   ```

### Post Hydration Detail

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/feed.ts`
**Lines**: 101-134

```typescript
async getPosts(
  uris: string[],
  includeTakedowns = false,
  given = new HydrationMap<Post>()
): Promise<Posts> {
  // 1. Fetch records from dataplane
  const res = await this.dataplane.getPostRecords({ uris: need })

  // 2. Parse each record
  return need.reduce((acc, uri, i) => {
    const record = parseRecord<PostRecord>(res.records[i], includeTakedowns)
    return acc.set(
      uri,
      record ? { ...record, ... } : null  // ← NULL if parseRecord fails!
    )
  }, base)
}
```

**CRITICAL FILTERING POINT #2**: `parseRecord()` can return `undefined`, causing post to become `null` in hydration state.

### parseRecord Filtering

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/util.ts`
**Lines**: 61-83

```typescript
export const parseRecord = <T>(
  entry: Record,
  includeTakedowns: boolean,
): RecordInfo<T> | undefined => {
  // Filter 1: Taken down records
  if (!includeTakedowns && entry.takenDown) {
    return undefined  // ← FILTERED OUT
  }

  // Filter 2: Parse record bytes
  const record = parseRecordBytes<T>(entry.record)
  const cid = entry.cid
  if (!record || !cid) return  // ← FILTERED OUT if no bytes or CID

  // Filter 3: Validate against lexicon schema
  if (!isValidRecord(record)) {
    return  // ← FILTERED OUT if schema validation fails
  }

  return { record, cid, sortedAt, indexedAt, takedownRef: safeTakedownRef(entry) }
}
```

**Filtering Conditions**:
1. **Takedown**: `entry.takenDown === true` and `includeTakedowns === false`
2. **Missing Data**: `entry.record` is empty or `entry.cid` is missing
3. **Invalid Schema**: Record doesn't match lexicon schema for its collection

---

## Stage 3: Filtering - Blocks and Mutes

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/api/app/bsky/feed/getAuthorFeed.ts`
**Lines**: 141-191

### Relationship Checks (lines 147-165)

```typescript
const relationship = hydration.profileViewers?.get(skeleton.actor.did)

// Throw error if viewer blocks author
if (relationship && (relationship.blocking || ctx.views.blockingByList(relationship, hydration))) {
  throw new InvalidRequestError(`Requester has blocked actor: ${skeleton.actor.did}`, 'BlockedActor')
}

// Throw error if author blocks viewer
if (relationship && (relationship.blockedBy || ctx.views.blockedByList(relationship, hydration))) {
  throw new InvalidRequestError(`Requester is blocked by actor: ${skeleton.actor.did}`, 'BlockedByActor')
}
```

### Item-Level Filtering (lines 167-188)

```typescript
const checkBlocksAndMutes = (item: FeedItem) => {
  const bam = ctx.views.feedItemBlocksAndMutes(item, hydration)
  return (
    !bam.authorBlocked &&
    !bam.originatorBlocked &&
    (!bam.authorMuted || bam.originatorMuted) // repost of muted content
  )
}

skeleton.items = skeleton.items.filter(checkBlocksAndMutes)
```

**Special Case - posts_and_author_threads** (lines 176-185):
```typescript
if (skeleton.filter === 'posts_and_author_threads') {
  const selfThread = new SelfThreadTracker(skeleton.items, hydration)
  skeleton.items = skeleton.items.filter((item) => {
    return (
      checkBlocksAndMutes(item) &&
      (item.repost || item.authorPinned || selfThread.ok(item.post.uri))
    )
  })
}
```

**CRITICAL FILTERING POINT #3**: Items filtered based on:
- Author is blocked by viewer
- Originator is blocked by viewer
- Author is muted (unless it's a repost)
- For `posts_and_author_threads`: Reply must be part of complete self-thread

---

## Stage 4: Presentation - Creating Views

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/api/app/bsky/feed/getAuthorFeed.ts`
**Lines**: 193-203

```typescript
const presentation = (inputs: {
  ctx: Context
  skeleton: Skeleton
  hydration: HydrationState
}) => {
  const { ctx, skeleton, hydration } = inputs
  const feed = mapDefined(skeleton.items, (item) =>
    ctx.views.feedViewPost(item, hydration),  // ← Returns undefined if post can't be viewed
  )
  return { feed, cursor: skeleton.cursor }
}
```

**CRITICAL FILTERING POINT #4**: `mapDefined()` filters out any `undefined` values returned by `feedViewPost()`

### feedViewPost Method

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/views/index.ts`
**Lines**: 955-980

```typescript
feedViewPost(
  item: FeedItem,
  state: HydrationState,
): Un$Typed<FeedViewPost> | undefined {
  const postInfo = state.posts?.get(item.post.uri)

  // Handle reposts (lines 963-968)
  let reason: $Typed<ReasonRepost> | $Typed<ReasonPin> | undefined
  if (item.repost) {
    const repost = state.reposts?.get(item.repost.uri)
    if (!repost) return  // ← FILTERED if repost not in hydration state
    if (repost.record.subject.uri !== item.post.uri) return  // ← FILTERED if repost subject mismatch
    reason = this.reasonRepost(item.repost.uri, repost, state)
    if (!reason) return  // ← FILTERED if reasonRepost returns undefined
  }

  // Create post view (lines 970-971)
  const post = this.post(item.post.uri, state)
  if (!post) return  // ← FILTERED if post() returns undefined

  // Create reply view (lines 972-974)
  const reply = !postInfo?.violatesThreadGate
    ? this.replyRef(item.post.uri, state)
    : undefined

  return { post, reason, reply }
}
```

**CRITICAL FILTERING POINT #5**: Returns `undefined` if:
1. Repost not found in hydration state
2. Repost subject URI doesn't match post URI
3. `reasonRepost()` returns undefined
4. `post()` returns undefined ← **MOST COMMON**

### post() Method - Primary View Creator

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/views/index.ts`
**Lines**: 897-953

```typescript
post(
  uri: string,
  state: HydrationState,
  depth = 0,
): Un$Typed<PostView> | undefined {
  // Check 1: Post in hydration state
  const post = state.posts?.get(uri)
  if (!post) return  // ← FILTERED: Post not in hydration state

  // Check 2: Author profile available
  const parsedUri = new AtUri(uri)
  const authorDid = parsedUri.hostname
  const author = this.profileBasic(authorDid, state)
  if (!author) return  // ← FILTERED: Author profile not available

  // Build post view
  const aggs = state.postAggs?.get(uri)
  const viewer = state.postViewers?.get(uri)
  const threadgateUri = postUriToThreadgateUri(uri)
  const labels = [
    ...(state.labels?.getBySubject(uri) ?? []),
    ...this.selfLabels({ uri, cid: post.cid, record: post.record }),
  ]

  return {
    uri,
    cid: post.cid,
    author,
    record: post.record,
    embed: depth < 2 && post.record.embed
      ? this.embed(uri, post.record.embed, state, depth + 1)
      : undefined,
    bookmarkCount: aggs?.bookmarks ?? 0,
    replyCount: aggs?.replies ?? 0,
    repostCount: aggs?.reposts ?? 0,
    likeCount: aggs?.likes ?? 0,
    quoteCount: aggs?.quotes ?? 0,
    indexedAt: this.indexedAt(post).toISOString(),
    viewer: viewer ? { ... } : undefined,
    labels,
    threadgate: !post.record.reply ? this.threadgate(threadgateUri, state) : undefined,
    debug: state.ctx?.includeDebugField ? { post: post.debug, author: author.debug } : undefined,
  }
}
```

**CRITICAL FILTERING POINTS #6 & #7**:
1. **Line 902-903**: Post not in `state.posts` hydration map
2. **Line 906-907**: Author profile not available via `profileBasic()`

---

## Complete Filtering Chain

Posts can be filtered at these points:

### Database Layer
1. **Dataplane INNER JOIN**: Post exists in `feed_item` but not in `post` table (or vice versa)

### Hydration Layer
2. **Takedown Status**: Post has `takedownRef` set and `includeTakedowns=false`
3. **Missing Record Bytes**: Post record has no bytes in database
4. **Missing CID**: Post record has no CID in database
5. **Schema Validation**: Post record doesn't match lexicon schema

### Filtering Layer
6. **Author Blocked**: Viewer has blocked the post author
7. **Originator Blocked**: Viewer has blocked the repost originator
8. **Author Muted**: Post author is muted (reposts exempted)
9. **Incomplete Self-Thread**: For `posts_and_author_threads` filter, reply is not part of complete self-thread

### Presentation Layer
10. **Post Not Hydrated**: Post not in `state.posts` (failed hydration earlier)
11. **Author Profile Missing**: Author's profile not in `state.actors`
12. **Repost Not Found**: Repost record not in `state.reposts`
13. **Repost Subject Mismatch**: Repost's subject URI doesn't match post URI

---

## Database Schema Requirements

For a post to successfully appear in author feed, it must have:

1. **record table** row with:
   - `uri` matching post URI
   - `json` field containing valid post record bytes
   - `cid` field containing valid CID
   - `takedownRef` either NULL or viewer must have `includeTakedowns=true`

2. **post table** row with:
   - `uri` matching post URI
   - `creator` matching author DID

3. **feed_item table** row with:
   - `postUri` matching post URI
   - `originatorDid` matching author DID
   - `sortAt` timestamp for pagination

4. **actor table** row with:
   - `did` matching post creator
   - `handle` set

5. **post_agg table** row (optional but expected):
   - `uri` matching post URI
   - Aggregates for likes, reposts, replies, quotes, bookmarks

---

## Pagination Mechanism

**Keyset Pagination** using `(sortAt, cid)` tuple:

```typescript
const keyset = new TimeCidKeyset(
  ref('feed_item.sortAt'),
  ref('feed_item.cid'),
)
```

**Cursor Format**: `{sortAt}::{cid}`

Example: `2025-10-30T18:29:22.800Z::bafyreiabc123...`

**Query Behavior**:
- First page: No cursor, returns top `limit` items ordered by `sortAt DESC, cid DESC`
- Next page: Cursor provides last item's `(sortAt, cid)`, query returns items BEFORE that point

---

## Key Files Reference

### AppView API Layer
- `/packages/bsky/src/api/app/bsky/feed/getAuthorFeed.ts` - Request handler, pipeline

### Dataplane Layer
- `/packages/bsky/src/data-plane/server/routes/feeds.ts` - Database queries
- `/packages/bsky/src/data-plane/server/routes/records.ts` - Record fetching
- `/packages/bsky/src/data-plane/server/db/pagination.ts` - Keyset pagination

### Hydration Layer
- `/packages/bsky/src/hydration/hydrator.ts` - Main hydration coordinator
- `/packages/bsky/src/hydration/feed.ts` - Post/feed hydration
- `/packages/bsky/src/hydration/actor.ts` - Actor/profile hydration
- `/packages/bsky/src/hydration/util.ts` - parseRecord, validation

### Views Layer
- `/packages/bsky/src/views/index.ts` - View generation (post, feedViewPost, etc.)

---

## Common Issues and Root Causes

### Issue: Post exists in database but not returned by API

**Possible Root Causes**:

1. **Database State**:
   - Post in `record` table but not in `post` table (INNER JOIN fails)
   - Post in `post` table but not in `feed_item` table (INNER JOIN fails)
   - Post has empty `json` field in `record` table
   - Post has NULL `cid` in `record` table

2. **Hydration Failures**:
   - Post has `takedownRef` set and viewer doesn't have `includeTakedowns`
   - Post record bytes don't match lexicon schema (validation fails)
   - Author profile record missing or invalid

3. **Filtering**:
   - Viewer has blocked post author
   - Post author has blocked viewer
   - Post author is muted by viewer
   - For `posts_and_author_threads`: Reply parent not in same feed

4. **Presentation**:
   - Post hydration succeeded but author profile hydration failed
   - Post in hydration state but `profileBasic()` returns undefined

---

## Environment Variables

### BSKY_INDEXED_AT_EPOCH

**Location**: Only in Bluesky's closed-source version
**Purpose**: Adjusts `indexedAt` timestamps for display purposes
**Effect in Open-Source**: None - this variable does NOT exist in open-source dataplane
**Common Misconception**: Does NOT affect filtering or post visibility

---

## Next Steps for Debugging

To diagnose why a specific post is filtered:

1. **Verify database state**:
   ```sql
   -- Check all relevant tables
   SELECT * FROM record WHERE uri = 'at://...';
   SELECT * FROM post WHERE uri = 'at://...';
   SELECT * FROM feed_item WHERE postUri = 'at://...';
   SELECT * FROM actor WHERE did = 'did:plc:...';
   ```

2. **Check record validity**:
   ```sql
   -- Verify record has bytes and CID
   SELECT uri, LENGTH(json::text) as json_length, cid, "takedownRef"
   FROM record WHERE uri = 'at://...';
   ```

3. **Trace hydration**:
   - Add logging to `getPosts()` in feed.ts to see if post hydrates
   - Add logging to `parseRecord()` to see which validation fails

4. **Trace presentation**:
   - Add logging to `post()` method to see if author profile found
   - Add logging to `feedViewPost()` to see which early return triggers

---

**End of Architecture Documentation**
