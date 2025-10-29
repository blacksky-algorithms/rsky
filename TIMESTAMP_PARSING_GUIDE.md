# Timestamp Parsing Implementation Guide

## Problem
The Rust indexer was passing `&str` types to PostgreSQL `TIMESTAMPTZ` columns, which caused type conversion errors. The postgres driver requires proper `DateTime<Utc>` types.

## Solution Pattern

For each plugin file, follow these steps:

### 1. Add chrono import
```rust
use chrono::{DateTime, Utc};
```

### 2. Add parse_timestamp helper function
Add this after the existing helper functions (like extract_creator):

```rust
/// Parse ISO8601/RFC3339 timestamp string to DateTime<Utc>
fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, IndexerError> {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| IndexerError::Serialization(format!("Invalid timestamp '{}': {}", timestamp, e)))
}
```

### 3. Update insert() method

**OLD CODE:**
```rust
// Extract createdAt from record
let created_at = record.get("createdAt").and_then(|c| c.as_str());

// Some logic...

client.execute(
    r#"INSERT INTO table_name (..., created_at, indexed_at, ...)
       VALUES ($1, $2, $3, ...)"#,
    &[..., &created_at, &timestamp, ...],
)
```

**NEW CODE:**
```rust
// Parse timestamps
let indexed_at = Self::parse_timestamp(timestamp)?;
let created_at = match record.get("createdAt").and_then(|c| c.as_str()) {
    Some(ts) => Self::parse_timestamp(ts)?,
    None => indexed_at.clone(),
};

// Some logic...

client.execute(
    r#"INSERT INTO table_name (..., created_at, indexed_at, ...)
       VALUES ($1, $2, $3, ...)"#,
    &[..., &created_at, &indexed_at, ...],
)
```

### 4. Replace ALL `&timestamp` with `&indexed_at` in execute() calls

This includes:
- Main INSERT statements
- Notification inserts
- Feed item inserts
- Any other database operations

## Files Needing Updates

- [x] follow.rs (reference implementation - completed)
- [ ] block.rs
- [ ] like.rs
- [ ] repost.rs
- [ ] post.rs (complex - has multiple execute calls)
- [ ] profile.rs
- [ ] feed_generator.rs
- [ ] list.rs
- [ ] starter_pack.rs
- [ ] thread_gate.rs
- [ ] post_gate.rs
- [ ] list_block.rs
- [ ] list_item.rs
- [ ] verification.rs
- [ ] labeler.rs (no created_at in record, just use indexed_at for both)

## Special Cases

### labeler.rs
This plugin doesn't have createdAt in the record, so just parse and use indexed_at:

```rust
let indexed_at = Self::parse_timestamp(timestamp)?;

client.execute(
    r#"INSERT INTO labeler (uri, cid, creator, created_at, indexed_at)
       VALUES ($1, $2, $3, $4, $5)"#,
    &[&uri, &cid, &creator, &indexed_at, &indexed_at],
)
```

### post.rs
Has multiple execute() calls for embeds, feed_item, notifications, etc. Make sure to replace ALL &timestamp references with &indexed_at.

## Testing
After updating all files:
```bash
cargo build -p rsky-indexer
cargo test -p rsky-integration-tests -- --ignored --nocapture
```

The test should pass with no timestamp conversion errors.
