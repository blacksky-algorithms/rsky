# Plugin Implementation Gap Analysis

Comprehensive analysis of gaps between Rust and TypeScript plugin implementations.

## Summary

- **Total Plugins:** 18
- **Complete:** 0
- **Needs Minor Fixes:** 6 (placeholder + simple CRUD)
- **Needs Major Work:** 12 (aggregates, notifications, complex logic)

---

## 1. PLACEHOLDER PLUGINS (3) - MINOR FIXES NEEDED

###  Status Plugin ✅ FIXED
- **Status:** Corrected to match TypeScript
- **Changes:** Removed table operations, only validates rkey='self'

### Chat Declaration Plugin
- **Gap:** Currently creates table, should only validate rkey='self'
- **Fix:** Remove table operations, add rkey validation

### Notification Declaration Plugin
- **Gap:** Currently creates table, should only validate rkey='self'
- **Fix:** Remove table operations, add rkey validation

---

## 2. SIMPLE CRUD PLUGINS (7) - MODERATE WORK

### Block Plugin
**Missing:**
- `actor_block` table (uses `block` table incorrectly)
- Fields: `creator`, `subjectDid`, `createdAt`, `indexedAt`
- Duplicate detection query

### Labeler Plugin
**Missing:**
- rkey='self' validation
- Fields: `creator`, `createdAt`, `indexedAt`

### List Block Plugin
**Missing:**
- Fields: `creator`, `subjectUri`, `createdAt`, `indexedAt`
- Duplicate detection query

### Starter Pack Plugin
**Missing:**
- Fields: `creator`, `name`, `createdAt`, `indexedAt`

### Feed Generator Plugin
**Missing:**
- Fields: `creator`, `feedDid`, `displayName`, `description`, `descriptionFacets`, `avatarCid`, `createdAt`, `indexedAt`

### List Plugin
**Missing:**
- Fields: `creator`, `name`, `purpose`, `description`, `descriptionFacets`, `avatarCid`, `createdAt`, `indexedAt`

### List Item Plugin
**Missing:**
- Fields: `creator`, `subjectDid`, `listUri`, `createdAt`, `indexedAt`
- listUri validation (same creator check)
- Duplicate detection query

---

## 3. GATE PLUGINS (2) - MODERATE WORK

### Thread Gate Plugin
**Missing:**
- Fields: `creator`, `postUri`, `createdAt`, `indexedAt`
- UPDATE post SET hasThreadGate = true WHERE uri = postUri
- postUri validation (creator/rkey match)
- Duplicate detection query
- DELETE: UPDATE post SET hasThreadGate = false

### Post Gate Plugin
**Missing:**
- Fields: `creator`, `postUri`, `createdAt`, `indexedAt`
- UPDATE post SET hasPostGate = true WHERE uri = postUri
- postUri validation (creator/rkey match)
- Duplicate detection query
- DELETE: UPDATE post SET hasPostGate = false

---

## 4. FEED INTERACTION PLUGINS (3) - MAJOR WORK

### Like Plugin
**Current:** Basic insert with creator, subject, created_at, indexed_at
**Missing:**
- `via` and `viaCid` fields
- Duplicate detection query (creator + subject)
- **Notifications:**
  - To subject author (reason: 'like')
  - To via author if exists (reason: 'like-via-repost')
  - Prevent self-notifications
- **Aggregate Updates:**
  - UPDATE post_agg SET likeCount = (SELECT COUNT(*) FROM like WHERE subject = ?)

### Repost Plugin
**Current:** Basic insert with creator, subject, created_at, indexed_at
**Missing:**
- `via` and `viaCid` fields
- Duplicate detection query (creator + subject)
- **feed_item table insert:**
  - type='repost', uri, cid, postUri (subject), originatorDid (creator)
  - sortAt = MIN(indexedAt, createdAt)
- **Notifications:**
  - To subject author (reason: 'repost')
  - To via author if exists (reason: 'repost-via-repost')
  - Prevent self-notifications
- **Aggregate Updates:**
  - UPDATE post_agg SET repostCount = (SELECT COUNT(*) FROM repost WHERE subject = ?)
- **DELETE:**
  - DELETE FROM feed_item WHERE uri = ?

### Follow Plugin
**Current:** Basic insert with creator, subject_did, created_at, indexed_at
**Missing:**
- Duplicate detection query (creator + subjectDid)
- **Notifications:**
  - To subject (reason: 'follow')
- **Aggregate Updates:**
  - INSERT/UPDATE profile_agg SET followersCount for subjectDid
  - INSERT/UPDATE profile_agg SET followsCount for creator (with transaction locking)

---

## 5. PROFILE PLUGIN - MODERATE WORK

### Profile Plugin
**Current:** Basic fields
**Missing:**
- `joinedViaStarterPackUri` field
- **Notifications:**
  - If joinedViaStarterPackUri exists: notify starter pack creator (reason: 'starterpack-joined')

---

## 6. VERIFICATION PLUGIN - MAJOR WORK

### Verification Plugin
**Missing:**
- Fields: `rkey`, `creator`, `subject`, `handle`, `displayName`, `createdAt`, `indexedAt`
- Duplicate detection query (subject + creator)
- **Notifications:**
  - On insert: notify subject (reason: 'verified')
  - On delete: notify subject (reason: 'unverified') with current timestamp

---

## 7. POST PLUGIN - MASSIVE WORK (Most Complex)

### Post Plugin
**Current:** Only uri, cid, creator, text, created_at, indexed_at
**Missing 90% of functionality:**

#### Core Fields:
- `replyRoot`, `replyRootCid`
- `replyParent`, `replyParentCid`
- `langs` (JSON array)
- `tags` (JSON array)
- `invalidReplyRoot` (boolean)
- `violatesThreadGate` (boolean)
- `violatesEmbeddingRules` (boolean)

#### Facet Extraction:
- Extract mentions from facets (isMention) -> store DIDs in post
- Extract links from facets (isLink) -> store URIs in post

#### Embed Tables:
- **post_embed_image:** postUri, position, imageCid, alt
- **post_embed_external:** postUri, uri, title, description, thumbCid
- **post_embed_record:** postUri, embedUri, embedCid
- **post_embed_video:** postUri, videoCid, alt
- **quote table:** If embedUri is a post -> uri, cid, subject, subjectCid, createdAt, indexedAt

#### feed_item Table:
- type='post', uri, cid, postUri, originatorDid, sortAt

#### Validation Logic:
- Reply validation (check if replyRoot/replyParent exist)
- Threadgate validation (check if reply violates threadgate)
- Quote validation (check if quoted post allows quoting via postgate)

#### Notifications:
- **Mentions:** To each mentioned DID (reason: 'mention')
- **Quotes:** To quoted post author (reason: 'quote') if not violating rules
- **Replies:**
  - To direct parent (reason: 'reply') if within depth 5
  - To ancestors up to depth 5
  - Respect threadgate hidden replies
  - Prevent duplicate notifications
  - Handle out-of-order indexing

#### Aggregate Updates:
- If replyParent exists: UPDATE post_agg SET replyCount for parent
- UPDATE profile_agg SET postsCount for creator (with transaction locking)
- If quote deleted: UPDATE post_agg SET quoteCount for embedUri

#### Delete Operations:
- DELETE FROM post
- DELETE FROM feed_item WHERE postUri = ?
- DELETE FROM quote WHERE subject = ?
- DELETE FROM post_embed_image WHERE postUri = ?
- DELETE FROM post_embed_external WHERE postUri = ?
- DELETE FROM post_embed_record WHERE postUri = ?
- DELETE FROM post_embed_video WHERE postUri = ?

---

## Implementation Priority

### Phase 1 - Quick Wins (Est: 2 hours)
1. ✅ Status (DONE)
2. Chat Declaration
3. Notification Declaration
4. Labeler
5. Block
6. List Block
7. Starter Pack

### Phase 2 - Moderate Complexity (Est: 4 hours)
8. Feed Generator
9. List
10. List Item
11. Thread Gate
12. Post Gate
13. Profile

### Phase 3 - Complex (Est: 6 hours)
14. Like (aggregates + notifications)
15. Repost (aggregates + notifications + feed_item)
16. Follow (aggregates + notifications)
17. Verification (notifications)

### Phase 4 - Most Complex (Est: 8+ hours)
18. Post (everything)

---

## Tables That Need Creation

Many aggregate and supporting tables are missing:

- `post_agg` (likeCount, repostCount, replyCount, quoteCount)
- `profile_agg` (followersCount, followsCount, postsCount)
- `feed_item` (for feed generation)
- `quote` (for quote posts)
- `post_embed_image`, `post_embed_external`, `post_embed_record`, `post_embed_video`
- `notification` (for all notification logic)

These need schema definitions in README.md and test setup.
