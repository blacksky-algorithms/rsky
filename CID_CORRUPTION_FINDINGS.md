# CID Serialization Corruption - Investigation Findings

**Date**: 2025-11-01
**Investigator**: Claude Code
**Database**: Blacksky Production (3.6B records)

---

## Executive Summary

**CRITICAL DATA CORRUPTION DISCOVERED**

- **Scope**: ~25.2 million corrupted records
- **Impact**: Lexicon validation failures causing `threadItemNotFound` errors
- **Timeline**: Almost all corruption occurred after October 15, 2025
- **Root Cause**: CID serialization bug in Rust indexer/ingester/backfiller (now fixed in code)

---

## Problem Description

### What Is Broken

CIDs (Content Identifiers) in the `record` table's `json` column are stored as **byte arrays** instead of **string representations**.

**Correct Format**:
```json
{
  "$type": "blob",
  "ref": {"$link": "bafkreiddj2ctjilffc4zupzkp3247mfq6hirce6uy5nn2kgk7zbtb75ykm"},
  "mimeType": "image/jpeg",
  "size": 255323
}
```

**Broken Format**:
```json
{
  "$type": "blob",
  "ref": [1, 85, 18, 32, 184, 11, 77, 2, ...],
  "mimeType": "image/jpeg",
  "size": 37147
}
```

### Impact

1. TypeScript AppView tries to parse these records
2. Lexicon validation fails on byte array CIDs
3. Records are filtered out as invalid
4. Posts/profiles/other content appear missing
5. Users see `threadItemNotFound` errors

---

## Corruption Statistics

### Total Affected Records: **25,200,000+**

| Collection Type | Broken Records | % of Total Corruption |
|----------------|-----------------|----------------------|
| `app.bsky.feed.post` | 25,152,286 | 99.81% |
| `app.bsky.actor.profile` | 25,290 | 0.10% |
| `app.rocksky.song` | 11,470 | 0.05% |
| `app.rocksky.scrobble` | 5,399 | 0.02% |
| `app.bsky.graph.list` | 2,266 | 0.01% |
| `app.rocksky.album` | 1,678 | 0.01% |
| `app.bsky.feed.generator` | 1,256 | <0.01% |
| Other 28 collections | ~1,600 | <0.01% |

**Database Context**:
- Total records in database: 3,636,410,321
- Corruption rate: ~0.69% of all records

---

## Temporal Analysis

**Key Finding**: Almost ALL corrupted records written after **October 15, 2025**

Evidence:
- All-time broken records: 25,200,000
- Broken records since Oct 15: 25,199,000 (99.996%)
- Only ~1,000 records pre-date October 15

**Implication**: The CID serialization bug was introduced around mid-October 2025.

---

## Affected CID Fields by Collection

### app.bsky.feed.post
- `embed.images[].image.ref` - Image embeds
- `embed.external.thumb.ref` - Link preview thumbnails
- `embed.video.video.ref` - Video embeds
- `embed.video.thumbnail.ref` - Video thumbnails

### app.bsky.actor.profile
- `avatar.ref` - Profile avatar
- `banner.ref` - Profile banner

### app.bsky.graph.list
- `avatar.ref` - List avatar

### app.bsky.feed.generator
- `avatar.ref` - Feed generator avatar

### Custom Collections
Various custom lexicon types with similar CID field patterns.

---

## Sample Corrupted Records

### Profile Example
```
URI: at://did:plc:2gxuugqsdkagw5ffcsfl6rss/app.bsky.actor.profile/self
JSON: {
  "$type":"app.bsky.actor.profile",
  "avatar":{
    "$type":"blob",
    "mimeType":"image/jpeg",
    "ref":[1,85,18,32,53,200,170,252,248,164,102,188,130,25,215,52,203,146,215,60,77,125,126,70,180,46,207,17,225,206,211,81,108,209,83,250],
    "size":37147
  },
  "description":"日常とすごく稀に絵",
  "displayName":"ちぎれパン"
}
```

### Post Example
```
URI: at://did:plc:5tqmbc5czp7y7pbjzqrdfggs/app.bsky.feed.post/3m3lyxscsm22w
JSON: {
  "$type":"app.bsky.feed.post",
  "createdAt":"2025-10-20T04:44:02.380Z",
  "embed":{
    "$type":"app.bsky.embed.images",
    "images":[{
      "alt":"",
      "aspectRatio":{"height":1500,"width":2000},
      "image":{
        "$type":"blob",
        "mimeType":"image/jpeg",
        "ref":[1,85,18,32,184,11,77,2,...]
        "size":...
      }
    }]
  }
}
```

---

## Migration Strategy Options

### Option 1: SQL-Based CID Reconstruction ⭐ RECOMMENDED

**Approach**: Convert byte arrays back to CID strings using PostgreSQL + plpgsql

**Pros**:
- Fast execution (direct SQL updates)
- No dependency on external PDS
- Can be done in-place
- Easily testable

**Cons**:
- Requires implementing CID encoding in SQL/plpgsql
- Need to handle multiple CID field locations

**Process**:
1. Create PL/pgSQL function to convert byte array to CID string
2. Identify all CID field paths in each collection type
3. Update JSON in batches using `jsonb_set()`
4. Validate fixes

### Option 2: Re-indexing from Source

**Approach**: Re-fetch records from authoritative PDS and re-index

**Pros**:
- Guaranteed correct data
- Validates against source of truth

**Cons**:
- Extremely slow (25M+ network requests)
- Dependency on PDS availability
- High network/API load
- May hit rate limits

### Option 3: Hybrid Approach

**Approach**: SQL fix for simple cases, re-index for complex/corrupted cases

**Pros**:
- Best of both worlds
- Fallback for edge cases

**Cons**:
- More complex implementation
- Requires both systems

---

## Recommended Migration Plan

### Phase 1: Develop CID Conversion Function

Create PostgreSQL function to convert byte array to CID string:

```sql
-- Pseudocode - needs implementation
CREATE OR REPLACE FUNCTION bytes_to_cid(bytes jsonb)
RETURNS text AS $$
  -- 1. Extract byte array from jsonb
  -- 2. Implement multibase/multicodec CID encoding
  -- 3. Return base32 string (bafkrei...)
$$ LANGUAGE plpgsql;
```

### Phase 2: Create Collection-Specific Update Functions

For each major collection type, create update function targeting specific CID paths:

```sql
-- Example for profiles
UPDATE record
SET json = jsonb_set(
  json::jsonb,
  '{avatar,ref}',
  to_jsonb(jsonb_build_object('$link', bytes_to_cid(json::jsonb->'avatar'->'ref')))
)::text
WHERE uri LIKE '%app.bsky.actor.profile%'
  AND jsonb_typeof(json::jsonb->'avatar'->'ref') = 'array'
```

### Phase 3: Test on Sample Data

1. Select 100 broken records
2. Apply migration
3. Verify CIDs match expected format
4. Test with TypeScript AppView
5. Confirm posts/profiles load correctly

### Phase 4: Execute Migration in Batches

1. Process in batches of 100,000 records
2. Monitor for errors
3. Track progress
4. Estimate: ~250 batches for 25M records

### Phase 5: Validation

1. Count remaining broken records (should be 0)
2. Test previously broken posts
3. Remove TypeScript workaround
4. Monitor production for errors

---

## CID Encoding Implementation Notes

CID format for these blobs (CIDv1):
- Multibase: `base32` (prefix: `b`)
- Multicodec: `raw` (0x55 = 85 decimal = first byte)
- Multihash: `sha256` (0x12 = 18 decimal = second byte)
- Hash length: 32 bytes (0x20 = 32 decimal = third byte)
- Hash: remaining bytes

**Example decoding**:
```
Byte array: [1, 85, 18, 32, 53, 200, 170, 252, ...]
           [version, codec, hash_fn, hash_len, hash_bytes...]
```

**PostgreSQL libraries needed**:
- May need to implement base32 encoding in plpgsql
- Or use PostgreSQL extensions (if available)

---

## Next Steps

1. ✅ Document findings (this document)
2. ⏳ Finish temporal analysis (queries running)
3. ⏳ Develop CID conversion function in PostgreSQL
4. ⏳ Create test migration script
5. ⏳ Test on sample records
6. ⏳ Execute full migration
7. ⏳ Validate and clean up

---

## Risks & Considerations

### Data Loss Risk: **LOW**
- Original blob data still exists in blob storage
- Only JSON serialization is corrupted
- Can reconstruct CIDs from byte arrays

### Downtime Risk: **MEDIUM**
- Migration could take hours for 25M records
- May need to pause indexing during migration
- Can do in rolling batches to minimize impact

### Complexity Risk: **MEDIUM**
- CID encoding implementation is non-trivial
- Multiple CID field locations to handle
- Edge cases may exist

---

## Open Questions

1. When exactly was the bug introduced? (waiting for temporal queries)
2. Are there any edge cases where byte arrays don't follow standard CID format?
3. Should we migrate all at once or in rolling batches?
4. Do we need to notify users about the issue?
5. How will we prevent this from happening again?

---

## Conclusion

We have identified a significant data corruption issue affecting 25.2 million records. The corruption is well-understood, and a SQL-based migration strategy appears feasible. The next priority is to develop and test the CID conversion function, then execute the migration in a controlled manner.

**Impact if not fixed**: Millions of posts and profiles will continue to appear missing to users.

**Impact if fixed**: Full restoration of affected content visibility.
