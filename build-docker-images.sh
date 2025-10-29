#!/bin/bash
set -e

echo "Building Rust Docker images for production..."

# Build from the workspace root
cd "$(dirname "$0")"

# Build rsky-ingester
echo "Building rsky-ingester..."
docker build -f rsky-ingester/Dockerfile -t rsky-ingester:latest .

# Build rsky-indexer
echo "Building rsky-indexer..."
docker build -f rsky-indexer/Dockerfile -t rsky-indexer:latest .

# Build rsky-backfiller
echo "Building rsky-backfiller..."
docker build -f rsky-backfiller/Dockerfile -t rsky-backfiller:latest .

echo ""
echo "✅ All images built successfully!"
echo ""
echo "Images:"
echo "  - rsky-ingester:latest"
echo "  - rsky-indexer:latest"
echo "  - rsky-backfiller:latest"
echo ""
echo "To deploy:"
echo "  docker-compose -f docker-compose.prod-rust.yml up -d"
