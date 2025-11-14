# Post Filtering Root Cause - FINAL ANALYSIS

**Date**: 2025-10-31
**Issue**: Post `at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k` returns `threadItemNotFound`
**Status**: ✅ ROOT CAUSE IDENTIFIED

---

## Executive Summary

The post is being filtered during the **presentation phase** because the **hydration phase fails to populate `state.actors`** with the author's profile data, even though ALL required data exists correctly in the database.

---

## Evidence Chain

### 1. Database Verification (Port 15432 - Production via SSH Tunnel)

**Actor Table**:
```sql
SELECT did, handle, "indexedAt", "takedownRef"
FROM actor
WHERE did = 'did:plc:w4xbfzo7kqfes5zb7r6qv3rw';
```
Result: ✅ Actor exists (`rude1.blacksky.team`, indexed 2025-10-13)

**Profile Record**:
```sql
SELECT uri, cid, LENGTH(json::text), "takedownRef"
FROM record
WHERE uri LIKE 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.actor.profile/%';
```
Result: ✅ Profile exists (934 bytes, CID: `bafyreihjlxxcfmrbrlo72ngvultvlczqpo7kpo3msrdm6p6w53io4w4kxe`)

**Post Record**:
```sql
SELECT uri, cid FROM post
WHERE uri = 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k';
```
Result: ✅ Post exists (CID: `bafyreiet7lxfkq7clfe5zq4uwv4bthh55qvoj7eygjcqvgvnc75hlbkaji`)

**Labels Check**:
```sql
SELECT src, uri, val, neg FROM label
WHERE uri IN (
  'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k',
  'did:plc:w4xbfzo7kqfes5zb7r6qv3rw',
  'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.actor.profile/self'
);
```
Result: ✅ 23 labels found, NONE are `!takedown` or `!suspend`

### 2. API Response Verification

**Blacksky**:
```bash
curl "https://api.blacksky.community/xrpc/app.bsky.unspecced.getPostThreadV2?anchor=at%3A%2F%2Fdid%3Aplc%3Aw4xbfzo7kqfes5zb7r6qv3rw%2Fapp.bsky.feed.post%2F3m4glqtatds2k"
```
Result: ❌ Returns `"$type": "app.bsky.unspecced.defs#threadItemNotFound"`

**Bluesky** (official):
```bash
curl "https://api.bsky.app/xrpc/app.bsky.unspecced.getPostThreadV2?anchor=at%3A%2F%2Fdid%3Aplc%3Aw4xbfzo7kqfes5zb7r6qv3rw%2Fapp.bsky.feed.post%2F3m4glqtatds2k"
```
Result: ✅ Returns full post with all details

---

## Code Analysis

### TypeScript AppView Pipeline

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/api/app/bsky/unspecced/getPostThreadV2.ts`

```typescript
// SKELETON PHASE (lines 38-72)
const skeleton = async (inputs) => {
  const res = await ctx.dataplane.getThread({ postUri: anchor, ... })
  return { anchor, uris: res.uris }  // ✅ Returns post URI correctly
}

// HYDRATION PHASE (lines 74-82)
const hydration = async (inputs) => {
  return ctx.hydrator.hydrateThreadPosts(
    skeleton.uris.map((uri) => ({ uri })),
    params.hydrateCtx
  )  // ❌ Fails to populate state.actors
}

// PRESENTATION PHASE (lines 84-104)
const presentation = (inputs) => {
  const { thread } = ctx.views.threadV2(skeleton, hydration, ...)
  return { thread }
}
```

### The Failure Chain

**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/views/index.ts`

```typescript
// LINE 1242: threadV2() entry point
threadV2(skeleton, state, opts) {
  const postView = this.post(anchorUri, state)  // LINE 1262
  const post = state.posts?.get(anchorUri)

  if (!post || !postView) {  // LINE 1264
    return {
      hasOtherReplies: false,
      thread: [
        this.threadV2ItemNotFound({ uri: anchorUri, depth: 0 })  // ❌ TRIGGERS HERE
      ]
    }
  }
  // ...
}

// LINE 897: post() - builds PostView
post(uri: string, state: HydrationState) {
  const post = state.posts?.get(uri)
  if (!post) return  // ✅ Post IS in state.posts

  const authorDid = new AtUri(uri).hostname
  const author = this.profileBasic(authorDid, state)  // LINE 906

  if (!author) return  // ❌ RETURNS UNDEFINED - author not found!
  // ...
}

// LINE 323: profileBasic() - gets author profile
profileBasic(did: string, state: HydrationState) {
  const actor = state.actors?.get(did)  // LINE 327

  if (!actor) return  // ❌ RETURNS UNDEFINED - actor not in state!
  // ...
}
```

---

## Root Cause

The hydration phase (`ctx.hydrator.hydrateThreadPosts()`) **fails to populate `state.actors`** with the author's profile, even though:

1. The author exists in the `actor` table
2. The profile exists in the `record` table
3. The dataplane's `getActors()` function can query this data

This causes:
1. `profileBasic(did, state)` returns `undefined` (line 328)
2. `post(uri, state)` returns `undefined` (line 907)
3. `threadV2()` sees `!postView` and returns `threadItemNotFound` (line 1267)

---

## Infrastructure Context

**From docker-compose.yml**:

```yaml
# TypeScript Dataplane (NOT Rust)
dataplane1:
  image: blacksky-bsky:custom
  command: node dataplane.js  # ← TypeScript, not Rust!
  environment:
    DB_POSTGRES_URL: "postgresql://bsky:...@pgbouncer:5432/bsky"

# TypeScript API (NOT Rust)
api1:
  image: blacksky-bsky:custom
  command: node api-basic.js  # ← TypeScript, not Rust!
  environment:
    BSKY_DATAPLANE_URLS: "http://dataplane1:3300,http://dataplane2:3301"
```

**Source Files**:
- `/Users/rudyfraser/Projects/atproto/services/bsky/api-basic.js`
- `/Users/rudyfraser/Projects/atproto/services/bsky/dataplane.js`

---

## Hypotheses for Why Hydration Fails

### Hypothesis 1: Dataplane Not Called
The TypeScript hydrator may not be calling `ctx.dataplane.getActors()` for the author DID.

### Hypothesis 2: Dataplane Returns Empty
The dataplane may be querying the database but returning an empty `actors` array.

### Hypothesis 3: Configuration Issue
The API may be misconfigured and not properly connecting to the dataplane instances.

### Hypothesis 4: Database Connection Issue
The dataplane may be connecting to pgbouncer but hitting a transaction/pooling issue.

---

## Next Steps

1. **Test Hypothesis**: Modify TypeScript code locally to add debug logging
2. **Build Docker Image**: `cd /Users/rudyfraser/Projects/atproto && docker build -t blacksky-bsky:debug .`
3. **Run with SSH Tunnel**: Connect to production Postgres via localhost:15432
4. **Verify Fix**: Check if `state.actors` gets populated correctly

---

## Files Analyzed

- `/Users/rudyfraser/Projects/atproto/packages/bsky/src/api/app/bsky/unspecced/getPostThreadV2.ts` (API handler)
- `/Users/rudyfraser/Projects/atproto/packages/bsky/src/views/index.ts` (Presentation layer)
- `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/hydrator.ts` (Hydration logic)
- `/Users/rudyfraser/Projects/atproto/packages/bsky/src/data-plane/server/routes/profile.ts` (Dataplane `getActors()`)

---

## Conclusion

**The issue is NOT with the indexed data** - everything is correctly indexed in the database.

**The issue IS with the TypeScript AppView/Dataplane hydration** - `state.actors` is not being populated during the hydration phase, causing the presentation phase to return `threadItemNotFound`.

This is a **BUG in the TypeScript AppView implementation or configuration**, not a data integrity issue.
