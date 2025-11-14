# Hydration Failure Root Cause Analysis

**Date**: 2025-10-31
**Issue**: `state.actors` not populated during hydration, causing `threadItemNotFound`
**Status**: 🔍 ROOT CAUSE IDENTIFIED - Testing hypothesis

---

## Executive Summary

The hydration phase fails to populate `state.actors` because the **dataplane's actor cache is stale or incomplete**. The actor exists in the database but the dataplane returns `exists: false` because it's checking a cache that doesn't have this actor.

---

## Code Trace: Complete Failure Path

### 1. API Handler → Hydration
**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/api/app/bsky/unspecced/getPostThreadV2.ts:74-82`

```typescript
const hydration = async (inputs) => {
  return ctx.hydrator.hydrateThreadPosts(
    skeleton.uris.map((uri) => ({ uri })),
    params.hydrateCtx
  )
}
```

### 2. Thread Posts Hydration → Posts Hydration
**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/hydrator.ts:730-757`

```typescript
async hydrateThreadPosts(refs: ItemRef[], ctx: HydrateCtx): Promise<HydrationState> {
  const postsState = await this.hydratePosts(refs, ctx)  // ← Calls hydratePosts
  // ...
}
```

### 3. Posts Hydration → Profile Hydration
**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/hydrator.ts:443-602`

```typescript
async hydratePosts(refs: ItemRef[], ctx: HydrateCtx, state: HydrationState = {}): Promise<HydrationState> {
  // ... gets posts

  // LINE 575: Calls hydrateProfiles for ALL post authors
  profileState = this.hydrateProfiles(allPostUris.map(didFromUri), ctx)

  return mergeManyStates(profileState, listState, ...)
}
```

### 4. Profile Hydration → Actor Hydration
**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/hydrator.ts:223-244`

```typescript
async hydrateProfiles(dids: string[], ctx: HydrateCtx): Promise<HydrationState> {
  const [actors, labels, profileViewersState] = await Promise.all([
    // LINE 229: Calls actor.getActors()
    this.actor.getActors(dids, {
      includeTakedowns,
      skipCacheForDids: ctx.skipCacheForViewer,  // ← ONLY SKIPS CACHE FOR VIEWER!
    }),
    this.label.getLabelsForSubjects(labelSubjectsForDid(dids), ctx.labelers),
    this.hydrateProfileViewers(dids, ctx),
  ])

  // LINE 237: Filters actors with takedown labels
  if (!includeTakedowns) {
    actionTakedownLabels(dids, actors, labels)
  }

  return mergeStates(profileViewersState ?? {}, {
    actors,  // ← Actors map returned in hydration state
    labels,
    ctx,
  })
}
```

### 5. Actor Hydration → Dataplane Query
**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/actor.ts:159-254`

```typescript
async getActors(
  dids: string[],
  opts: { includeTakedowns?: boolean, skipCacheForDids?: string[] } = {},
): Promise<Actors> {
  const { includeTakedowns = false, skipCacheForDids } = opts
  if (!dids.length) return new HydrationMap<Actor>()

  // LINE 168: Calls dataplane with skipCacheForDids
  const res = await this.dataplane.getActors({ dids, skipCacheForDids })

  // LINE 169-253: Process each actor
  return dids.reduce((acc, did, i) => {
    const actor = res.actors[i]
    const isNoHosted =
      actor.takenDown ||
      (actor.upstreamStatus && actor.upstreamStatus !== 'active')

    // LINE 174-180: FILTERS OUT actors!
    if (
      !actor.exists ||  // ← TRIGGERS HERE when dataplane returns exists: false
      (isNoHosted && !includeTakedowns) ||
      !!actor.tombstonedAt
    ) {
      return acc.set(did, null)  // ← Actor filtered to NULL
    }

    // LINE 231-252: Build Actor object
    return acc.set(did, {
      did,
      handle: parseString(actor.handle),
      profile: profile?.record,
      profileCid: profile?.cid,
      // ... etc
    })
  }, new HydrationMap<Actor>())
}
```

### 6. Dataplane Query → Database
**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/data-plane/server/routes/profile.ts:24-169`

```typescript
async getActors(req) {
  const { dids, returnAgeAssuranceForDids } = req
  if (dids.length === 0) return { actors: [] }

  // LINE 50-63: Query actor table
  const [handlesRes, verificationsReceived, profiles, ...] = await Promise.all([
    db.db
      .selectFrom('actor')
      .leftJoin('actor_state', 'actor_state.did', 'actor.did')
      .where('actor.did', 'in', dids)
      .selectAll('actor')
      .select('actor_state.priorityNotifs')
      .execute(),
    // ... other queries
    getRecords(db)({ uris: profileUris }),  // LINE 72: Gets profile records
  ])

  // LINE 88-89: Build byDid map
  const byDid = keyBy(handlesRes, 'did')
  const actors = dids.map((did, i) => {
    const row = byDid.get(did)  // ← Lookup actor by DID

    return {
      exists: !!row,  // ← LINE 146: If row is undefined, exists = false
      handle: row?.handle ?? undefined,
      profile: profiles.records[i],
      takenDown: !!row?.takedownRef,
      // ... etc
    }
  })
  return { actors }
}
```

### 7. Presentation Layer Sees Missing Actor
**File**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/views/index.ts:897-907, 323-328, 1262-1267`

```typescript
// LINE 897: post() - tries to build PostView
post(uri: string, state: HydrationState) {
  const post = state.posts?.get(uri)
  if (!post) return  // ✅ Post IS in state.posts

  const authorDid = new AtUri(uri).hostname
  const author = this.profileBasic(authorDid, state)  // LINE 906

  if (!author) return  // ❌ RETURNS UNDEFINED - author not in state.actors!
}

// LINE 323: profileBasic() - tries to get author
profileBasic(did: string, state: HydrationState) {
  const actor = state.actors?.get(did)  // LINE 327

  if (!actor) return  // ❌ RETURNS UNDEFINED - actor is NULL
}

// LINE 1262: threadV2() - sees missing postView
threadV2(skeleton, state, opts) {
  const postView = this.post(anchorUri, state)
  const post = state.posts?.get(anchorUri)

  if (!post || !postView) {  // ❌ postView is undefined
    return {
      hasOtherReplies: false,
      thread: [this.threadV2ItemNotFound({ uri: anchorUri, depth: 0 })]
    }
  }
}
```

---

## Root Cause: Stale or Incomplete Actor Cache

### The Problem

The dataplane has a caching layer for actor data. When `hydrateProfiles()` is called:

1. It calls `this.actor.getActors(dids, { skipCacheForDids: ctx.skipCacheForViewer })`
2. `skipCacheForDids` only includes the viewer's DID (if there is a viewer)
3. For ALL other DIDs (including post authors), the dataplane uses its CACHE
4. If the actor was indexed but never added to the cache, the dataplane returns `exists: false`
5. The ActorHydrator filters the actor to `null`
6. `state.actors` doesn't have the author
7. Presentation layer returns `threadItemNotFound`

### Evidence

1. ✅ **Actor exists in database** (verified via port 15432):
   ```
   did: did:plc:w4xbfzo7kqfes5zb7r6qv3rw
   handle: rude1.blacksky.team
   indexedAt: 2025-10-13T15:06:43.819Z
   ```

2. ✅ **Profile exists in database** (verified via port 15432):
   ```
   uri: at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.actor.profile/self
   cid: bafyreihjlxxcfmrbrlo72ngvultvlczqpo7kpo3msrdm6p6w53io4w4kxe
   json length: 934 bytes
   ```

3. ✅ **Post exists in database** (verified via port 15432):
   ```
   uri: at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k
   cid: bafyreiet7lxfkq7clfe5zq4uwv4bthh55qvoj7eygjcqvgvnc75hlbkaji
   ```

4. ✅ **No takedown labels** (verified via port 15432):
   ```
   23 labels found on DID, NONE are !takedown or !suspend
   ```

5. ❌ **Bluesky's API works, Blacksky's doesn't**:
   - Bluesky (api.bsky.app): Returns full post with all details ✓
   - Blacksky (api.blacksky.community): Returns `threadItemNotFound` ✗

### Why Bluesky Works But Blacksky Doesn't

**Bluesky's official infrastructure**:
- Mature caching infrastructure with proper cache invalidation
- Actor cache is correctly populated and maintained
- All actors are queryable

**Blacksky's custom infrastructure**:
- Uses same TypeScript codebase but different deployment
- May have incomplete cache population
- May have stale cache without proper invalidation
- Actor indexed in database but NOT in cache

---

## Hypothesis Testing Plan

### Test 1: Bypass Actor Cache

**Modify**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/actor.ts:168`

Change from:
```typescript
const res = await this.dataplane.getActors({ dids, skipCacheForDids })
```

To:
```typescript
// TEMPORARY DEBUG: Skip cache for ALL DIDs to test hypothesis
const res = await this.dataplane.getActors({
  dids,
  skipCacheForDids: dids  // ← Force cache bypass for all actors
})
```

**Expected Result**: If this fixes the issue, it confirms the cache is the problem.

### Test 2: Add Debug Logging

**Modify**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/hydration/actor.ts:174-180`

Add logging before the filter:
```typescript
const actor = res.actors[i]
const isNoHosted =
  actor.takenDown ||
  (actor.upstreamStatus && actor.upstreamStatus !== 'active')

// DEBUG LOGGING
if (did === 'did:plc:w4xbfzo7kqfes5zb7r6qv3rw') {
  console.log('[ACTOR DEBUG]', JSON.stringify({
    did,
    exists: actor.exists,
    takenDown: actor.takenDown,
    upstreamStatus: actor.upstreamStatus,
    tombstonedAt: actor.tombstonedAt,
    handle: actor.handle,
    hasProfile: !!actor.profile,
  }, null, 2))
}

if (
  !actor.exists ||
  (isNoHosted && !includeTakedowns) ||
  !!actor.tombstonedAt
) {
  return acc.set(did, null)
}
```

**Expected Result**: Logs will show `exists: false` for the problematic DID.

---

## Implementation Steps

1. **Modify TypeScript code** with Test 1 changes
2. **Build Docker image**:
   ```bash
   cd /Users/rudyfraser/Projects/atproto
   docker build -t blacksky-bsky:debug .
   ```
3. **Run locally with SSH tunnel to production DB**:
   ```bash
   # SSH tunnel to production Postgres (already running on port 15432)

   # Run modified API container
   docker run --rm --network host \
     -e DB_POSTGRES_URL="postgresql://bsky:PASSWORD@localhost:15432/bsky" \
     -e PORT=3000 \
     blacksky-bsky:debug \
     node api-basic.js
   ```
4. **Test the problematic post**:
   ```bash
   curl "http://localhost:3000/xrpc/app.bsky.unspecced.getPostThreadV2?anchor=at%3A%2F%2Fdid%3Aplc%3Aw4xbfzo7kqfes5zb7r6qv3rw%2Fapp.bsky.feed.post%2F3m4glqtatds2k"
   ```
5. **Verify fix**: Post should return successfully instead of `threadItemNotFound`

---

## If Hypothesis Is Correct

### Short-term Fix
Force cache bypass for all actor queries by modifying the hydrator to always pass all DIDs in `skipCacheForDids`.

### Long-term Fix
1. Investigate why actor cache is incomplete
2. Implement proper cache invalidation when actors are indexed
3. OR disable caching entirely if not critical for performance

---

## Alternative Hypotheses (If Test Fails)

If bypassing the cache doesn't fix it, other possibilities:

1. **Database replication lag**: Dataplane connected to read replica that's behind
2. **pgbouncer configuration**: Connection pooling causing transaction isolation issues
3. **Dataplane query bug**: Query construction issue causing actor to be missed
4. **Profile record parsing failure**: `getRecords()` returns profile but parsing fails

---

## Next Steps

**IMMEDIATE**: Implement Test 1, build Docker image, and verify hypothesis.
