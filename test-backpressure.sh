#!/bin/bash

# Test script to verify backpressure mechanism in rsky-ingester
# This script sets a very low high water mark to trigger backpressure immediately
# and monitors memory usage and Redis stream length

set -e

echo "=========================================="
echo "rsky-ingester Backpressure Test"
echo "=========================================="
echo ""

# Check if Redis is running
if ! redis-cli ping > /dev/null 2>&1; then
    echo "ERROR: Redis is not running at localhost:6379"
    echo "Please start Redis: docker-compose up redis -d"
    exit 1
fi

echo "✓ Redis is running"
echo ""

# Clean Redis streams
echo "Cleaning Redis streams..."
redis-cli DEL firehose_live firehose_live:cursor:relay1.us-east.bsky.network > /dev/null
echo "✓ Redis streams cleaned"
echo ""

# Set test configuration
export INGESTER_HIGH_WATER_MARK=100
export INGESTER_BATCH_SIZE=50
export INGESTER_BATCH_TIMEOUT_MS=1000
export REDIS_URL=redis://localhost:6379
export INGESTER_RELAY_HOSTS=relay1.us-east.bsky.network
export INGESTER_MODE=firehose
export RUST_LOG=info

echo "Test Configuration:"
echo "  HIGH_WATER_MARK: $INGESTER_HIGH_WATER_MARK"
echo "  BATCH_SIZE: $INGESTER_BATCH_SIZE"
echo "  RELAY: $INGESTER_RELAY_HOSTS"
echo ""

# Start monitoring in background
echo "Starting Redis stream monitor..."
(
    while true; do
        STREAM_LEN=$(redis-cli XLEN firehose_live 2>/dev/null || echo 0)
        echo "[$(date '+%H:%M:%S')] Redis stream length: $STREAM_LEN"

        if [ "$STREAM_LEN" -gt "$INGESTER_HIGH_WATER_MARK" ]; then
            echo "  ⚠️  BACKPRESSURE should be triggered!"
        fi

        sleep 5
    done
) &
MONITOR_PID=$!

# Cleanup function
cleanup() {
    echo ""
    echo "Stopping monitor..."
    kill $MONITOR_PID 2>/dev/null || true

    echo "Cleaning up Redis streams..."
    redis-cli DEL firehose_live firehose_live:cursor:relay1.us-east.bsky.network > /dev/null

    echo ""
    echo "Test completed."
    exit 0
}

trap cleanup INT TERM

echo "=========================================="
echo "Starting ingester (press Ctrl+C to stop)"
echo "=========================================="
echo ""
echo "Expected behavior:"
echo "  1. Ingester starts consuming from firehose"
echo "  2. Redis stream fills up to ~100 entries"
echo "  3. Backpressure warning appears in logs"
echo "  4. Stream length stays around 100"
echo "  5. Memory metrics show bounded in-flight events"
echo "  6. Memory usage stays stable (no growth)"
echo ""
echo "Watch for logs like:"
echo '  - "Backpressure active: stream_len=XXX, high_water=100, events_in_memory=YYY"'
echo '  - "Memory metrics: X events in-flight"'
echo ""

# Build and run ingester
cargo build --bin ingester && cargo run --bin ingester

# Should not reach here unless ingester exits
cleanup
