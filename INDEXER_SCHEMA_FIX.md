# Indexer Schema Mismatch - URGENT FIX

## Problem

Indexers are stuck with 12M+ events in Redis streams:
- `firehose_live`: 1,564,120
- `repo_backfill`: 851,025
- `firehose_backfill`: **11,061,589** ← CRITICAL
- `label_live`: 1,000,051

**Error:**
```
ERROR: column "indexed_at" of relation "label" does not exist
```

The Rust indexer uses `indexed_at` but the database has a different column name.

## Immediate Diagnostic

Run this on your production database to check the label table schema:

```bash
# SSH to production server
ssh blacksky@api

# Check actual column names in label table
docker exec -i blackfill-postgres psql -U postgres -d bsky_local -c "\d label"
```

**OR** if using pgbouncer:

```bash
psql -h localhost -p 15433 -U postgres -d bsky_local -c "\d label"
```

## Expected vs Actual Schema

### Rust Code Expects (label_indexer.rs:202):
```sql
INSERT INTO label (src, uri, cid, val, cts, indexed_at)
VALUES ($1, $2, $3, $4, $5, NOW())
ON CONFLICT (src, uri, val) DO UPDATE
SET cid = EXCLUDED.cid,
    cts = EXCLUDED.cts,
    indexed_at = EXCLUDED.indexed_at
```

### Likely Actual Schema (TypeScript version):
```sql
-- The column is probably named "createdAt" not "indexed_at"
CREATE TABLE label (
    src TEXT NOT NULL,
    uri TEXT NOT NULL,
    cid TEXT,
    val TEXT NOT NULL,
    cts TIMESTAMPTZ NOT NULL,
    createdAt TIMESTAMPTZ NOT NULL,  ← This is the actual column name
    PRIMARY KEY (src, uri, val)
);
```

## Quick Fix Options

### Option 1: Fix the Rust Code (RECOMMENDED)

If database has `createdAt`, update label_indexer.rs:

```rust
// Change line 202 from:
INSERT INTO label (src, uri, cid, val, cts, indexed_at)

// To:
INSERT INTO label (src, uri, cid, val, cts, "createdAt")

// And line 207 from:
indexed_at = EXCLUDED.indexed_at

// To:
"createdAt" = EXCLUDED."createdAt"
```

### Option 2: Add Missing Column to Database

If you want to keep the Rust code as-is:

```sql
-- Add the indexed_at column
ALTER TABLE label ADD COLUMN indexed_at TIMESTAMPTZ;

-- Backfill existing rows
UPDATE label SET indexed_at = "createdAt" WHERE indexed_at IS NULL;

-- Make it NOT NULL
ALTER TABLE label ALTER COLUMN indexed_at SET NOT NULL;

-- Update default
ALTER TABLE label ALTER COLUMN indexed_at SET DEFAULT NOW();
```

## Recommended Action Plan

1. **Check database schema** (run diagnostic above)
2. **Fix the code** to match database column name
3. **Rebuild indexer** Docker image
4. **Restart indexers**
5. **Monitor stream drainage**

##Files to Fix

If database uses `createdAt`:
- `rsky-indexer/src/label_indexer.rs:202` - Change column name
- `rsky-indexer/src/label_indexer.rs:207` - Change column name in ON CONFLICT

## Build and Deploy

```bash
# On local machine
cd /Users/rudyfraser/Projects/rsky
# Make the fix
# Then build
docker build -t rsky-indexer:fixed -f Dockerfile.indexer .

# Tag for production
docker tag rsky-indexer:fixed your-registry/rsky-indexer:latest

# Push to registry
docker push your-registry/rsky-indexer:latest

# On production server
docker-compose -f docker-compose.prod-rust.yml pull
docker-compose -f docker-compose.prod-rust.yml restart indexer1 indexer2 indexer3 indexer4 indexer5 indexer6
```

## Verification

After restart, streams should start draining:

```bash
# Monitor in real-time
watch -n 2 '
echo "Firehose Live:     $(docker exec backfill-redis redis-cli XLEN firehose_live)"
echo "Repo Backfill:     $(docker exec backfill-redis redis-cli XLEN repo_backfill)"
echo "Firehose Backfill: $(docker exec backfill-redis redis-cli XLEN firehose_backfill)"
echo "Label Live:        $(docker exec backfill-redis redis-cli XLEN label_live)"
'
```

Numbers should be **decreasing** now, not stuck.

## Additional Notes

This same issue may affect other tables. After fixing label, check logs for similar errors on other tables and apply the same pattern.
