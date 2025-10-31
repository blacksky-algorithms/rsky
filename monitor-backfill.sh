#!/bin/bash

# Backfill Pipeline Monitor
# Shows real-time status of the entire backfill pipeline

set -e

REDIS_HOST="${REDIS_HOST:-localhost}"
REDIS_PORT="${REDIS_PORT:-6380}"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo "=========================================="
echo "  rsky Backfill Pipeline Monitor"
echo "  $(date)"
echo "=========================================="

# Function to format numbers with commas
format_number() {
    printf "%'d" "$1" 2>/dev/null || echo "$1"
}

# Function to calculate rate
calculate_rate() {
    local current=$1
    local previous=$2
    local seconds=$3
    echo $(( (current - previous) / seconds ))
}

echo -e "\n${BLUE}📊 REDIS STREAM LENGTHS${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Get stream lengths
REPO_BACKFILL=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN repo_backfill 2>/dev/null || echo "0")
FIREHOSE_BACKFILL=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN firehose_backfill 2>/dev/null || echo "0")
FIREHOSE_LIVE=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN firehose_live 2>/dev/null || echo "0")
LABELS_LIVE=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN labels_live 2>/dev/null || echo "0")

printf "%-25s %15s\n" "repo_backfill:" "$(format_number $REPO_BACKFILL)"
printf "%-25s %15s\n" "firehose_backfill:" "$(format_number $FIREHOSE_BACKFILL)"
printf "%-25s %15s\n" "firehose_live:" "$(format_number $FIREHOSE_LIVE)"
printf "%-25s %15s\n" "labels_live:" "$(format_number $LABELS_LIVE)"

echo -e "\n${BLUE}👥 CONSUMER GROUP STATUS${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Check firehose_backfill consumer group
echo -e "\n${YELLOW}firehose_backfill → firehose_group${NC}"
PENDING_BACKFILL=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XPENDING firehose_backfill firehose_group 2>/dev/null | head -1)
echo "  Pending messages: $PENDING_BACKFILL"

# Get active consumers (idle < 5 seconds)
redis-cli -h $REDIS_HOST -p $REDIS_PORT XINFO CONSUMERS firehose_backfill firehose_group 2>/dev/null | \
    awk '/^name$/ {getline; name=$1; for(i=0;i<5;i++) getline; if($1 < 5000) print "  ✓ " name " (active)"}' | head -10

# Check repo_backfill consumer group
echo -e "\n${YELLOW}repo_backfill → repo_backfill_group${NC}"
PENDING_REPO=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XPENDING repo_backfill repo_backfill_group 2>/dev/null | head -1)
echo "  Pending messages: $PENDING_REPO"

echo -e "\n${BLUE}📈 PROCESSING RATES${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Calculate rates over 10 seconds
FIREHOSE_BACKFILL_START=$FIREHOSE_BACKFILL
sleep 10
FIREHOSE_BACKFILL_END=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT XLEN firehose_backfill 2>/dev/null || echo "$FIREHOSE_BACKFILL")

CHANGE=$((FIREHOSE_BACKFILL_START - FIREHOSE_BACKFILL_END))
RATE=$((CHANGE / 10))

if [ $CHANGE -gt 0 ]; then
    echo -e "${GREEN}firehose_backfill draining: -$CHANGE messages in 10s (~$RATE msg/s)${NC}"

    # Estimate time to drain
    if [ $RATE -gt 0 ]; then
        SECONDS_TO_DRAIN=$((FIREHOSE_BACKFILL_END / RATE))
        HOURS=$((SECONDS_TO_DRAIN / 3600))
        MINUTES=$(((SECONDS_TO_DRAIN % 3600) / 60))
        echo "  Estimated time to drain: ${HOURS}h ${MINUTES}m"
    fi
elif [ $CHANGE -lt 0 ]; then
    POSITIVE_CHANGE=$((-CHANGE))
    POSITIVE_RATE=$((-RATE))
    echo -e "${YELLOW}firehose_backfill growing: +$POSITIVE_CHANGE messages in 10s (+$POSITIVE_RATE msg/s)${NC}"
else
    echo -e "${RED}firehose_backfill static: no change${NC}"
fi

echo -e "\n${BLUE}🔍 BACKFILLER METRICS${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Try to fetch backfiller metrics (assumes port 9090)
if command -v curl &> /dev/null; then
    BACKFILLER_METRICS=$(curl -s http://localhost:9090/metrics 2>/dev/null || echo "")

    if [ -n "$BACKFILLER_METRICS" ]; then
        REPOS_PROCESSED=$(echo "$BACKFILLER_METRICS" | grep "backfiller_repos_processed_total" | grep -v "#" | awk '{print $2}' | head -1)
        REPOS_FAILED=$(echo "$BACKFILLER_METRICS" | grep "backfiller_repos_failed_total" | grep -v "#" | awk '{print $2}' | head -1)
        RECORDS_EXTRACTED=$(echo "$BACKFILLER_METRICS" | grep "backfiller_records_extracted_total" | grep -v "#" | awk '{print $2}' | head -1)

        printf "%-25s %15s\n" "Repos processed:" "$(format_number ${REPOS_PROCESSED:-0})"
        printf "%-25s %15s\n" "Repos failed:" "$(format_number ${REPOS_FAILED:-0})"
        printf "%-25s %15s\n" "Records extracted:" "$(format_number ${RECORDS_EXTRACTED:-0})"
    else
        echo "⚠ Backfiller metrics not available (http://localhost:9090/metrics)"
    fi
else
    echo "⚠ curl not found, skipping backfiller metrics"
fi

echo ""
echo "=========================================="
echo "  Run 'watch -n 5 $0' for live monitoring"
echo "=========================================="
