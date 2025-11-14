# NULL thumbCid Root Cause Analysis

**Date**: 2025-10-31
**Issue**: 6.8 million posts have NULL thumbCid in post_embed_external table
**Impact**: Posts with external link embeds may be filtered or rendered incorrectly by AppView

---

## Problem Statement

**Observed Behavior**:
- Post `at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k` exists in database but filtered from API
- Bluesky's official AppView (blacksky.community) displays the post correctly ✓
- Blacksky's AppView (staging.blacksky.community) returns "post not found" ❌
- Database query shows 6,824,503 posts with NULL thumbCid in post_embed_external table

---

## Root Cause

**Location**: `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/post.rs` lines 99-102

**Buggy Code**:
```rust
let thumb_cid = external
    .get("thumb")
    .and_then(|t| t.get("ref"))
    .and_then(|r| r.as_str());  // ← BUG: ref is byte array, not string!
```

**The Problem**:
1. The blob `ref` field in the JSON is a **byte array**: `[1, 85, 18, 32, 99, 78, 133, 52, ...]`
2. The code calls `.as_str()` expecting it to be a JSON string
3. `.as_str()` returns `None` for byte arrays
4. `thumb_cid` is set to `NULL` in the database for ALL external embeds with thumbnails

**Correct TypeScript Implementation** (atproto/packages/bsky/src/data-plane/server/indexing/plugins/post.ts:186):
```typescript
thumbCid: external.thumb?.ref.toString() || null,
```

The TypeScript `.toString()` method properly converts the blob bytes to a CID string.

---

## Post JSON Structure

**Database Format** (what rsky-indexer sees):
```json
{
  "thumb": {
    "ref": [1, 85, 18, 32, 99, 78, 133, 52, 161, 101, 40, 185, 154, 63, 42, 126, 245, 207, 176, 176, 241, 209, 17, 19, 212, 199, 90, 221, 40, 202, 254, 67, 48, 255, 184, 83],
    "size": 255323,
    "$type": "blob",
    "mimeType": "image/jpeg"
  }
}
```

**Expected CID** (from byte array conversion):
- Byte array: `[1, 85, 18, 32, 99, 78, 133, 52, ...]`
- CID string: `bafkreiddj2ctjilffc4zupzkp3247mfq6hirce6uy5nn2kgk7zbtb75ykm`

---

## CID Format Explanation

CID (Content Identifier) structure:
- **Byte 0**: Version (1 = CIDv1)
- **Byte 1**: Codec (0x55 = raw)
- **Byte 2**: Multihash type (0x12 = SHA-256)
- **Byte 3**: Hash length (32 bytes)
- **Bytes 4-35**: The actual hash

CIDv1 encoding:
- Uses base32 encoding (RFC4648, lowercase, no padding)
- Prefixed with 'b' (multibase identifier for base32)
- Example: `bafkreiddj2ctjilffc4zupzkp3247mfq6hirce6uy5nn2kgk7zbtb75ykm`

---

## Fix Applied (Manual Database Update)

Updated three specific posts for testing:

```sql
UPDATE post_embed_external
SET "thumbCid" = 'bafkreiddj2ctjilffc4zupzkp3247mfq6hirce6uy5nn2kgk7zbtb75ykm'
WHERE "postUri" = 'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4glqtatds2k';

UPDATE post_embed_external
SET "thumbCid" = 'bafkreicavwrg3wq5xer52de7dzbjhx2fdi44juxvak7bgtka42y4f2ijci'
WHERE "postUri" IN (
  'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4bhkwjmls2i',
  'at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.post/3m4e626zbmk2p'
);
```

**Verification**: All three posts now have correct thumbCid values matching the blob refs in their JSON.

---

## Required Fix in rsky-indexer

The indexer needs to properly convert blob ref byte arrays to CID strings.

**Recommended Implementation**:

```rust
use cid::Cid;
use multihash::Multihash;

// In the external embed handling code:
let thumb_cid = external
    .get("thumb")
    .and_then(|t| t.get("ref"))
    .and_then(|r| {
        if let Some(arr) = r.as_array() {
            // Convert JSON array to Vec<u8>
            let bytes: Vec<u8> = arr
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as u8))
                .collect();

            if bytes.len() >= 4 {
                // Parse as CID
                match Cid::try_from(&bytes[..]) {
                    Ok(cid) => Some(cid.to_string()),
                    Err(_) => None,
                }
            } else {
                None
            }
        } else {
            // Fallback: if ref is already a string (shouldn't happen but handle gracefully)
            r.as_str().map(String::from)
        }
    });
```

**Dependencies Required**:
- `cid = "0.11"` (or latest version)
- `multihash = "0.19"` (or compatible version)

---

## Impact on Production

**Affected Posts**: 6,824,503 posts with external embeds containing thumbnails

**Possible Issues**:
1. Posts might be filtered from timelines if AppView requires thumbCid
2. Thumbnails not displayed for external links
3. API responses incomplete for external embed data

**Immediate Fix**: The manual database updates for the three test posts should allow us to verify if this resolves the post visibility issue.

**Long-term Fix**:
1. Fix the rsky-indexer code to properly convert blob refs to CIDs
2. Run a migration to backfill the 6.8M NULL thumbCid values:
   - Extract blob ref bytes from record.json for each post
   - Convert to CID string
   - Update post_embed_external.thumbCid

---

## Testing

**Conversion Script**: `/tmp/bytes_to_cid.py`
- Converts byte arrays to CID strings using Python stdlib
- Verified against user-provided CID for post 3m4glqtatds2k
- Successfully converted all three test posts

**Test Posts**:
1. `3m4glqtatds2k` → `bafkreiddj2ctjilffc4zupzkp3247mfq6hirce6uy5nn2kgk7zbtb75ykm`
2. `3m4bhkwjmls2i` → `bafkreicavwrg3wq5xer52de7dzbjhx2fdi44juxvak7bgtka42y4f2ijci`
3. `3m4e626zbmk2p` → `bafkreicavwrg3wq5xer52de7dzbjhx2fdi44juxvak7bgtka42y4f2ijci` (same as #2)

---

## Next Steps

1. **Verify Fix**: Test if the post now appears on staging.blacksky.community
2. **Fix Indexer**: Implement proper CID conversion in rsky-indexer code
3. **Backfill Script**: Create migration to fix existing 6.8M NULL thumbCid entries
4. **Deploy**: Rebuild and deploy fixed indexer to production
5. **Monitor**: Watch for new external embeds to ensure thumbCid is populated correctly

---

## Related Files

- **Indexer Bug**: `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/post.rs:99-102`
- **TypeScript Reference**: `/Users/rudyfraser/Projects/atproto/packages/bsky/src/data-plane/server/indexing/plugins/post.ts:186`
- **Conversion Script**: `/tmp/bytes_to_cid.py`
- **Test Post JSONs**: `/tmp/three_posts.csv`
- **Root Cause Doc**: `POST_FILTERING_ROOT_CAUSE.md`
- **Architecture Doc**: `APPVIEW_ARCHITECTURE.md`
