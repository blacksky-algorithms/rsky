#!/bin/bash

# Test script for rsky-indexer against production DB/Redis via SSH tunnels
# Uses a SEPARATE consumer group to avoid interfering with production

set -e

echo "🚀 Starting rsky-indexer TEST against production (via SSH tunnels)"
echo ""
echo "Configuration:"
echo "  Database: localhost:15433 (prod via SSH tunnel)"
echo "  Redis: localhost:6380 (prod via SSH tunnel)"
echo "  Consumer Group: TEST_rust_indexer (separate from prod)"
echo "  Concurrency: 10 (low for testing)"
echo "  Pool Size: 20"
echo ""
echo "⚠️  This will READ from production queues but use a SEPARATE consumer group"
echo "⚠️  Messages will be processed and written to production database"
echo ""
read -p "Press Enter to continue or Ctrl+C to abort..."

export RUST_LOG="info,rsky_indexer=debug"
export RUST_BACKTRACE="1"

# Redis (via SSH tunnel to production)
export REDIS_URL="redis://localhost:6380"

# Database (via SSH tunnel to production PGBouncer)
export DATABASE_URL="postgresql://bsky:BEVoNPm7z0lT5tMAv6hF5SQUMkIQBTRHhx0JiKjxCsdVTR274zxdPw5o9CGtpmgh@localhost:15433/bsky"

# Indexer configuration - SEPARATE from production!
export INDEXER_STREAMS="firehose_live,firehose_backfill"
export INDEXER_GROUP="firehose_group"           # Same group as prod (will compete for messages)
export INDEXER_CONSUMER="TEST_rust_indexer_1"   # Different consumer name
export INDEXER_CONCURRENCY="10"                 # Low concurrency for testing
export INDEXER_BATCH_SIZE="100"                 # Smaller batches

# Connection pool - our fixed values
export DB_POOL_MAX_SIZE="20"
export DB_POOL_MIN_IDLE="5"

# Indexer mode
export INDEXER_MODE="stream"  # Only stream indexer, not label

# DID resolution (optional, can disable for faster testing)
export ENABLE_DID_RESOLUTION="true"

echo ""
echo "Starting indexer..."
echo ""

# Run the indexer
exec ./target/release/indexer
