#!/bin/bash
# Verification script for rsky-indexer production deployment
# Success criteria: Redis streams draining, PostgreSQL records increasing

set -e

REDIS_HOST="localhost"
REDIS_PORT="6380"
PG_HOST="localhost"
PG_PORT="15433"
PG_USER="bsky"
PG_DB="bsky"

echo "=========================================="
echo "rsky-indexer Production Verification"
echo "=========================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check 1: Redis Stream Lengths
echo "1. Checking Redis Stream Lengths..."
FIREHOSE_LIVE=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN firehose_live)
FIREHOSE_BACKFILL=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN firehose_backfill)
LABEL_LIVE=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN label_live)

echo "   firehose_live:     $FIREHOSE_LIVE"
echo "   firehose_backfill: $FIREHOSE_BACKFILL"
echo "   label_live:        $LABEL_LIVE"

# Wait 10 seconds and check again
echo ""
echo "   Waiting 10 seconds to measure drain rate..."
sleep 10

FIREHOSE_LIVE_AFTER=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN firehose_live)
FIREHOSE_BACKFILL_AFTER=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN firehose_backfill)
LABEL_LIVE_AFTER=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN label_live)

LIVE_DIFF=$((FIREHOSE_LIVE - FIREHOSE_LIVE_AFTER))
BACKFILL_DIFF=$((FIREHOSE_BACKFILL - FIREHOSE_BACKFILL_AFTER))
LABEL_DIFF=$((LABEL_LIVE - LABEL_LIVE_AFTER))

echo ""
echo "   After 10 seconds:"
echo "   firehose_live:     $FIREHOSE_LIVE_AFTER (change: $LIVE_DIFF)"
echo "   firehose_backfill: $FIREHOSE_BACKFILL_AFTER (change: $BACKFILL_DIFF)"
echo "   label_live:        $LABEL_LIVE_AFTER (change: $LABEL_DIFF)"

STREAM_CHECK="FAIL"
if [ $LIVE_DIFF -gt 0 ] || [ $BACKFILL_DIFF -gt 0 ]; then
    echo -e "   ${GREEN}✓ PASS${NC}: Streams are draining"
    STREAM_CHECK="PASS"

    # Calculate drain rate
    TOTAL_DRAINED=$((LIVE_DIFF + BACKFILL_DIFF))
    RATE_PER_SEC=$((TOTAL_DRAINED / 10))
    echo "   Drain rate: ~$RATE_PER_SEC messages/second"
else
    echo -e "   ${RED}✗ FAIL${NC}: Streams are NOT draining"
fi

echo ""

# Check 2: Consumer Group Activity
echo "2. Checking Consumer Group Activity..."
CONSUMER_INFO=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XINFO CONSUMERS firehose_live firehose_group)

# Parse consumer info to check inactive times
echo "$CONSUMER_INFO" | grep -A 1 "name" | grep -v "^--$" | paste -d " " - - | while read -r line; do
    CONSUMER_NAME=$(echo "$line" | grep -oP 'name.*?\K[^\s]+' | head -1)
    INACTIVE=$(echo "$line" | grep -oP 'inactive.*?\K\d+' | head -1)
    PENDING=$(echo "$line" | grep -oP 'pending.*?\K\d+' | head -1)

    if [ -n "$CONSUMER_NAME" ] && [ -n "$INACTIVE" ]; then
        if [ "$INACTIVE" -lt 10000 ]; then
            echo -e "   ${GREEN}✓${NC} $CONSUMER_NAME: inactive=${INACTIVE}ms, pending=$PENDING"
        else
            INACTIVE_SEC=$((INACTIVE / 1000))
            echo -e "   ${RED}✗${NC} $CONSUMER_NAME: inactive=${INACTIVE_SEC}s (TOO HIGH), pending=$PENDING"
        fi
    fi
done

echo ""

# Check 3: PostgreSQL Latest Records
echo "3. Checking PostgreSQL Latest Records..."
LATEST_POSTS=$(psql -h $PG_HOST -p $PG_PORT -U $PG_USER -d $PG_DB -t -c "SELECT uri, \"createdAt\", \"indexedAt\" FROM post ORDER BY \"indexedAt\" DESC LIMIT 3" 2>&1)

if [ $? -eq 0 ]; then
    echo "   Latest indexed posts:"
    echo "$LATEST_POSTS" | head -3

    # Check if any posts were indexed in the last hour
    RECENT_COUNT=$(psql -h $PG_HOST -p $PG_PORT -U $PG_USER -d $PG_DB -t -c "SELECT COUNT(*) FROM post WHERE \"indexedAt\" > NOW() - INTERVAL '1 hour'" 2>&1)

    if [ "$RECENT_COUNT" -gt 0 ]; then
        echo -e "   ${GREEN}✓ PASS${NC}: $RECENT_COUNT posts indexed in the last hour"
    else
        echo -e "   ${YELLOW}⚠ WARNING${NC}: No posts indexed in the last hour"
    fi
else
    echo -e "   ${RED}✗ FAIL${NC}: Could not query PostgreSQL"
    echo "   Error: $LATEST_POSTS"
fi

echo ""

# Summary
echo "=========================================="
echo "SUMMARY"
echo "=========================================="

if [ "$STREAM_CHECK" = "PASS" ]; then
    echo -e "${GREEN}✓ SUCCESS${NC}: rsky-indexer is consuming from Redis and draining streams"
    echo ""
    echo "Expected behavior:"
    echo "- Streams should continue draining at ~$RATE_PER_SEC msg/sec"
    echo "- PostgreSQL should show increasing record counts"
    echo "- Consumer inactive times should stay < 10 seconds"
    exit 0
else
    echo -e "${RED}✗ FAILURE${NC}: rsky-indexer is NOT working correctly"
    echo ""
    echo "Troubleshooting steps:"
    echo "1. Check Docker logs: docker logs --tail 100 rust-indexer1"
    echo "2. Verify consumer group position: redis-cli -h $REDIS_HOST -p $REDIS_PORT XINFO GROUPS firehose_live"
    echo "3. Check for errors: docker logs rust-indexer1 | grep ERROR"
    exit 1
fi
