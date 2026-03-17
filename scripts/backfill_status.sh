#!/bin/bash
# backfill_status.sh - Show current backfill progress
#
# Usage: bash scripts/backfill_status.sh
# Runs on the appview server (40.160.64.162)

set -euo pipefail

DB="postgresql://appview:2TP0Do4c50gK4O3OH3UwO9k5XX9oaQH3maP2rxWzZd0@localhost/appview_db?options=-csearch_path%3Dbsky"

echo "=== Blacksky Backfill Status ==="
echo "  $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
echo ""

# Database counts
echo "--- Record Counts ---"
ACTORS=$(psql "$DB" -t -A -c "SELECT COUNT(*) FROM actor;")
FOLLOWS=$(psql "$DB" -t -A -c "SELECT COUNT(*) FROM follow;")
POSTS=$(psql "$DB" -t -A -c "SELECT COUNT(*) FROM post;")
LIKES=$(psql "$DB" -t -A -c "SELECT COUNT(*) FROM \"like\";")
PROFILE_AGG=$(psql "$DB" -t -A -c "SELECT COUNT(*) FROM profile_agg;")

printf "  %-20s %15s / %15s  (%5.1f%%)\n" "Actors" "$ACTORS" "42,000,000" "$(echo "scale=1; $ACTORS * 100 / 42000000" | bc)"
printf "  %-20s %15s / %15s  (%5.1f%%)\n" "Follows" "$FOLLOWS" "3,400,000,000" "$(echo "scale=1; $FOLLOWS * 100 / 3400000000" | bc)"
printf "  %-20s %15s / %15s  (%5.1f%%)\n" "Posts" "$POSTS" "2,300,000,000" "$(echo "scale=1; $POSTS * 100 / 2300000000" | bc)"
printf "  %-20s %15s / %15s  (%5.1f%%)\n" "Likes" "$LIKES" "12,700,000,000" "$(echo "scale=1; $LIKES * 100 / 12700000000" | bc)"
printf "  %-20s %15s / %15s\n" "profile_agg" "$PROFILE_AGG" "42,000,000"
echo ""

# Queue status (if queue_backfill is available)
if command -v queue_backfill &>/dev/null; then
    echo "--- Queue Status ---"
    queue_backfill --db-path /data/backfill/backfill_cache status 2>/dev/null || echo "  (queue_backfill not available)"
    echo ""
fi

# Wintermute metrics
if curl -sf http://localhost:9090/metrics >/dev/null 2>&1; then
    echo "--- Wintermute Metrics ---"
    METRICS=$(curl -sf http://localhost:9090/metrics)

    backfiller_total=$(echo "$METRICS" | grep '^backfiller_repos_processed_total' | awk '{print $2}' || echo "N/A")
    backfiller_errors=$(echo "$METRICS" | grep '^backfiller_repos_error_total' | awk '{print $2}' || echo "N/A")
    indexer_events=$(echo "$METRICS" | grep '^indexer_events_total' | awk '{print $2}' || echo "N/A")

    printf "  %-35s %s\n" "Backfiller repos processed:" "$backfiller_total"
    printf "  %-35s %s\n" "Backfiller repos errors:" "$backfiller_errors"
    printf "  %-35s %s\n" "Indexer events total:" "$indexer_events"
    echo ""
fi

# Sample user comparison
echo "--- Sample User Follower Comparison ---"
echo "  (Blacksky DB vs public Bluesky API)"

SAMPLE_DIDS=(
    "did:plc:l55p7haox472tht3q46fvht6"
    "did:plc:w4xbfzo7kqfes5zb7r6qv3rw"
    "did:plc:kta7dqcqoamo5ixlajxbtjps"
)

for DID in "${SAMPLE_DIDS[@]}"; do
    # DB count
    DB_COUNT=$(psql "$DB" -t -A -c "SELECT COALESCE(\"followersCount\", 0) FROM profile_agg WHERE did = '$DID';" 2>/dev/null || echo "0")

    # Public API count
    API_RESP=$(curl -sf "https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile?actor=$DID" 2>/dev/null || echo "{}")
    API_COUNT=$(echo "$API_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin).get('followersCount', 'N/A'))" 2>/dev/null || echo "N/A")
    HANDLE=$(echo "$API_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin).get('handle', 'unknown'))" 2>/dev/null || echo "unknown")

    if [ "$DB_COUNT" != "0" ] && [ "$API_COUNT" != "N/A" ] && [ "$API_COUNT" != "0" ]; then
        PCT=$(echo "scale=1; $DB_COUNT * 100 / $API_COUNT" | bc 2>/dev/null || echo "?")
        printf "  @%-30s  DB: %8s  API: %8s  (%s%%)\n" "$HANDLE" "$DB_COUNT" "$API_COUNT" "$PCT"
    else
        printf "  @%-30s  DB: %8s  API: %8s\n" "$HANDLE" "$DB_COUNT" "$API_COUNT"
    fi
done
echo ""
