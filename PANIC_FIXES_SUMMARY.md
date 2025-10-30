# Production Panic Fixes Summary

## Overview
Fixed 5 critical panics discovered in production deployment of the Rust indexer.

## Fixes Applied

### Fix 1: Missing `seq` Field (rsky-indexer/src/lib.rs)
**Error**: `Serialization("missing field 'seq'")`
**Cause**: TypeScript sometimes omits seq field in Repo/Account/Identity events
**Solution**: Made seq field optional with `#[serde(default = "default_seq")]` returning SEQ_BACKFILL (-1)
```rust
#[serde(rename = "repo")]
Repo {
    #[serde(default = "default_seq")]
    seq: i64,
    // ...
}
```

### Fix 2: Timestamp Parsing Type Mismatch (rsky-indexer/src/indexing/mod.rs:389-394)
**Error**: `cannot convert between Rust type DateTime<Utc> and Postgres type varchar`
**Cause**: Tried to read varchar column directly as DateTime
**Solution**: Read as String first, then parse to DateTime
```rust
let indexed_at_str: Option<String> = row.get(1);
let indexed_at = parse_timestamp(&indexed_at_str)?;
```

### Fix 3: Reqwest TLS Provider (rsky-identity/Cargo.toml:15)
**Error**: `No provider set`
**Cause**: Feature flag `rustls-tls-webpki-roots-no-provider` disabled crypto
**Solution**: Changed to `rustls-tls-webpki-roots` with default provider

### Fix 4: Hickory DNS Runtime Conflict (rsky-identity/src/handle/mod.rs)
**Error**: `Cannot start a runtime from within a runtime`
**Cause**: Using sync `Resolver` that calls `block_on` from within tokio runtime
**Solution**: Switched to async `TokioAsyncResolver` with `.await` on all lookups
```rust
let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());
let results = resolver.txt_lookup(format!("{SUBDOMAIN}.{handle}")).await?;
```

### Fix 5: DateTime Serialization in index_handle (rsky-indexer/src/indexing/mod.rs:365)
**Error**: `cannot convert DateTime<Utc> to varchar` in index_handle function
**Cause**: Passing DateTime directly to SQL query for varchar column
**Solution**: Convert to RFC3339 string before database insert
```rust
&[&did, &verified_handle, &indexed_at.to_rfc3339()],
```

## Testing
All fixes have been:
- Compiled successfully with no errors
- Tested locally with TypeScript ingester
- Verified to produce no panics or ERROR-level logs
- Only expected WARN messages for invalid UTF-8 data (properly handled)

## Deployment Steps
```bash
# 1. Commit all changes (already done)
git log --oneline -5

# 2. Push to remote
git push origin rude1/backfill

# 3. On production server
cd /mnt/nvme/bsky/atproto
git pull origin rude1/backfill
docker build -f rsky-indexer/Dockerfile -t rsky-indexer:latest .
docker compose -f docker-compose.prod-rust.yml restart indexer1 indexer2 indexer3 indexer4 indexer5 indexer6

# 4. Monitor logs
docker compose -f docker-compose.prod-rust.yml logs -f indexer1 indexer2 indexer3 indexer4 indexer5 indexer6 | grep -E "ERROR|panic"
```

## Expected Outcome
- Zero panics
- Zero ERROR-level messages
- Only expected WARN messages for invalid UTF-8 bytes
- Stable processing of firehose events
- Queue lengths decreasing steadily

## Infrastructure Status
- Redis: 64GB maxmemory (from 8GB)
- DB Pool Size: 20 per indexer (from 200)
- 6 indexer containers: concurrency=10, batch_size=50
- Stream lengths: firehose_live: 19.6M, firehose_backfill: 30.6M
