# rsky-syntax Refactoring Guide

**Date**: 2025-10-31
**Mission**: Replace manual string parsing with rsky-syntax AtUri across rsky-indexer, rsky-ingester, and rsky-backfiller

## Overview

This guide documents all locations where manual string parsing should be replaced with the centralized rsky-syntax helpers that have been added to `rsky-indexer/src/indexing/mod.rs`.

### Centralized Helpers Available

Located in `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/mod.rs` (lines 46-70):

```rust
/// Extract DID from AT Protocol URI using rsky-syntax
pub fn extract_did_from_uri(uri: &str) -> Option<String> {
    rsky_syntax::aturi::AtUri::new(uri.to_string(), None)
        .ok()
        .map(|at_uri| at_uri.host)
}

/// Extract record key (rkey) from AT Protocol URI using rsky-syntax
pub fn extract_rkey_from_uri(uri: &str) -> Option<String> {
    rsky_syntax::aturi::AtUri::new(uri.to_string(), None)
        .ok()
        .map(|at_uri| at_uri.get_rkey())
        .filter(|rkey| !rkey.is_empty())
}

/// Extract collection from AT Protocol URI using rsky-syntax
pub fn extract_collection_from_uri(uri: &str) -> Option<String> {
    rsky_syntax::aturi::AtUri::new(uri.to_string(), None)
        .ok()
        .map(|at_uri| at_uri.get_collection())
        .filter(|collection| !collection.is_empty())
}
```

---

## Part 1: rsky-indexer Plugin Files

### Pattern 1: Replace Manual `extract_creator()` Function

**Current Pattern:**
```rust
fn extract_creator(uri: &str) -> Option<String> {
    if let Some(stripped) = uri.strip_prefix("at://") {
        if let Some(did_end) = stripped.find('/') {
            return Some(stripped[..did_end].to_string());
        }
    }
    None
}
```

**Replace With:**
```rust
fn extract_creator(uri: &str) -> Option<String> {
    crate::indexing::extract_did_from_uri(uri)
}
```

**Files to Update (15 files):**

1. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/post.rs`
   - Lines 14-21: Replace entire function

2. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/like.rs`
   - Lines 13-20: Replace entire function

3. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/follow.rs`
   - Lines 13-20: Replace entire function

4. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/repost.rs`
   - Lines 13-20: Replace entire function

5. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/block.rs`
   - Lines 13-20: Replace entire function

6. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/profile.rs`
   - Lines 13-20: Replace entire function

7. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/list.rs`
   - Lines 13-20: Replace entire function

8. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/list_item.rs`
   - Lines 13-20: Replace entire function

9. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/list_block.rs`
   - Lines 13-20: Replace entire function

10. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/feed_generator.rs`
    - Lines 13-20: Replace entire function

11. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/labeler.rs`
    - Lines 13-20: Replace entire function

12. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/starter_pack.rs`
    - Lines 13-20: Replace entire function

13. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/verification.rs`
    - Lines 13-20: Replace entire function

14. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/post_gate.rs`
    - Lines 13-21: Replace entire function

15. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/thread_gate.rs`
    - Lines 13-21: Replace entire function

---

### Pattern 2: Replace Manual `extract_rkey()` Function

**Current Pattern:**
```rust
fn extract_rkey(uri: &str) -> Option<String> {
    uri.rsplit('/').next().map(|s| s.to_string())
}
```

**Replace With:**
```rust
fn extract_rkey(uri: &str) -> Option<String> {
    crate::indexing::extract_rkey_from_uri(uri)
}
```

**Files to Update (8 files):**

1. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/profile.rs`
   - Lines 23-25: Replace entire function

2. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/labeler.rs`
   - Lines 23-25: Replace entire function

3. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/chat_declaration.rs`
   - Lines 12-14: Replace entire function

4. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/notif_declaration.rs`
   - Lines 12-14: Replace entire function

5. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/status.rs`
   - Lines 12-14: Replace entire function

6. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/post_gate.rs`
   - Lines 23-25: Replace entire function

7. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/thread_gate.rs`
   - Lines 23-25: Replace entire function

8. `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/verification.rs`
   - Lines 23-25: Replace entire function

---

### Pattern 3: Replace Manual URI Parsing in Core Module

**File:** `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/mod.rs`

**Location:** Lines 248-253 (in `delete_record` function)

**Current Code:**
```rust
pub async fn delete_record(&self, uri: &str, _rev: &str) -> Result<(), IndexerError> {
    // Parse URI to get collection
    let parts: Vec<&str> = uri.split('/').collect();
    if parts.len() < 3 {
        return Err(IndexerError::InvalidUri(uri.to_string()));
    }

    let collection = parts[parts.len() - 2];
    // ...
}
```

**Replace With:**
```rust
pub async fn delete_record(&self, uri: &str, _rev: &str) -> Result<(), IndexerError> {
    // Parse URI to get collection using rsky-syntax
    let collection = extract_collection_from_uri(uri)
        .ok_or_else(|| IndexerError::InvalidUri(uri.to_string()))?;

    // ...
}
```

---

## Part 2: rsky-backfiller

**File:** `/Users/rudyfraser/Projects/rsky/rsky-backfiller/src/repo_backfiller.rs`

**Location:** Lines 663-670 (in `process_repo` function)

**Current Code:**
```rust
for entry in chunk {
    // Parse key to get collection and rkey
    let parts: Vec<&str> = entry.key.split('/').collect();
    if parts.len() != 2 {
        warn!("Invalid data key: {}", entry.key);
        continue;
    }
    let collection = parts[0].to_string();
    let rkey = parts[1].to_string();
```

**Replace With:**
```rust
for entry in chunk {
    // Parse key to get collection and rkey
    // Keys in repo are in format: "collection/rkey" (not full AT URIs)
    let parts: Vec<&str> = entry.key.split('/').collect();
    if parts.len() != 2 {
        warn!("Invalid data key: {}", entry.key);
        continue;
    }
    let collection = parts[0].to_string();
    let rkey = parts[1].to_string();
```

**NOTE:** This instance is parsing repo internal keys, NOT AT URIs, so the current approach is actually correct! No change needed here.

---

## Part 3: rsky-indexer One-Off Mode

**File:** `/Users/rudyfraser/Projects/rsky/rsky-indexer/src/bin/indexer.rs`

**Location:** Lines 413-423 (in `run_one_off_indexing` function)

**Current Code:**
```rust
for entry in &leaves {
    // Parse key to get collection and rkey
    let parts: Vec<&str> = entry.key.split('/').collect();
    if parts.len() != 2 {
        warn!("Invalid data key: {}", entry.key);
        skipped_count += 1;
        continue;
    }
    let collection = parts[0].to_string();
    let rkey = parts[1].to_string();
```

**Replace With:**
```rust
for entry in &leaves {
    // Parse key to get collection and rkey
    // Keys in repo are in format: "collection/rkey" (not full AT URIs)
    let parts: Vec<&str> = entry.key.split('/').collect();
    if parts.len() != 2 {
        warn!("Invalid data key: {}", entry.key);
        skipped_count += 1;
        continue;
    }
    let collection = parts[0].to_string();
    let rkey = parts[1].to_string();
```

**NOTE:** Same as backfiller - this is parsing repo internal keys, NOT AT URIs. Current approach is correct! No change needed.

---

## Part 4: rsky-ingester

Need to search for similar patterns. Based on the architecture, ingester likely doesn't parse URIs much since it's just writing events to Redis streams.

**Action Required:** Search for manual URI parsing patterns:

```bash
cd ~/Projects/rsky/rsky-ingester
grep -n "split('/')" src/**/*.rs
grep -n "strip_prefix(\"at://\")" src/**/*.rs
grep -n "rsplit('/')" src/**/*.rs
```

If any found, apply the same pattern as above.

---

## Summary Statistics

### Changes Required

**rsky-indexer:**
- 15 files: Replace `extract_creator()` function
- 8 files: Replace `extract_rkey()` function
- 1 file: Replace manual split in `delete_record()`

**Total Plugin Files**: 18 files with function replacements
**Total Core Files**: 1 file with inline replacement

**rsky-backfiller:** No changes needed (parsing repo keys, not URIs)

**rsky-ingester:** To be determined after search

---

## Testing Checklist

After making changes:

1. **Build rsky-indexer:**
   ```bash
   cd ~/Projects/rsky
   cargo build --release --bin indexer
   ```

2. **Test with one-off indexing:**
   ```bash
   RUST_LOG=info DATABASE_URL="postgresql://..." \
     ./target/release/indexer --index-repo did:plc:w4xbfzo7kqfes5zb7r6qv3rw
   ```

3. **Run existing indexer test:**
   ```bash
   cargo test -p rsky-indexer
   ```

4. **Verify no behavioral changes:**
   - Check that DIDs are still extracted correctly
   - Check that rkeys are still extracted correctly
   - Check that collections are still extracted correctly

---

## Rationale

### Why This Change?

1. **Protocol Adherence**: Uses official AT Protocol URI parsing from rsky-syntax
2. **DRY Principle**: Single source of truth for URI parsing logic
3. **Maintainability**: Changes to URI format only need updates in rsky-syntax
4. **Correctness**: rsky-syntax handles edge cases and validation properly
5. **Consistency**: Same parsing approach across all crates

### What About Non-URI Parsing?

Some code parses **repo internal keys** (format: `collection/rkey`) which are NOT AT URIs. These should keep using `split('/')` because:
- They don't have the `at://did` prefix
- They're an internal repo storage format
- rsky-syntax AtUri parser expects full URIs

Examples of correct split usage:
- Repo leaf keys in backfiller: `app.bsky.feed.post/3kv5zfm...`
- Repo leaf keys in one-off indexer: `app.bsky.feed.like/3la8hqn...`

---

## Quick Reference: Before & After

### Helper Functions

**Before:**
```rust
fn extract_creator(uri: &str) -> Option<String> {
    if let Some(stripped) = uri.strip_prefix("at://") {
        if let Some(did_end) = stripped.find('/') {
            return Some(stripped[..did_end].to_string());
        }
    }
    None
}
```

**After:**
```rust
fn extract_creator(uri: &str) -> Option<String> {
    crate::indexing::extract_did_from_uri(uri)
}
```

### Inline Parsing

**Before:**
```rust
let parts: Vec<&str> = uri.split('/').collect();
if parts.len() < 3 {
    return Err(IndexerError::InvalidUri(uri.to_string()));
}
let collection = parts[parts.len() - 2];
```

**After:**
```rust
let collection = extract_collection_from_uri(uri)
    .ok_or_else(|| IndexerError::InvalidUri(uri.to_string()))?;
```

---

## Completion Checklist

- [ ] Update 15 plugin files with `extract_creator()` replacement
- [ ] Update 8 plugin files with `extract_rkey()` replacement
- [ ] Update `indexing/mod.rs` with collection parsing replacement
- [ ] Search rsky-ingester for manual parsing patterns
- [ ] Build all crates successfully
- [ ] Test one-off indexing with sample DID
- [ ] Update CLAUDE.md with mission completion
- [ ] Commit changes with descriptive message

---

**End of Refactoring Guide**
