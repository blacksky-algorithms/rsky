#!/bin/bash
# Setup script for integration tests

set -e

echo "Setting up integration test infrastructure..."

# Check if Redis is running
if ! redis-cli ping > /dev/null 2>&1; then
    echo "❌ Redis is not running!"
    echo "Please start Redis first:"
    echo "  docker-compose up -d redis"
    echo "  OR"
    echo "  redis-server"
    exit 1
fi
echo "✅ Redis is running"

# Check if PostgreSQL is running
if ! pg_isready -h localhost -p 5432 > /dev/null 2>&1; then
    echo "❌ PostgreSQL is not running!"
    echo "Please start PostgreSQL first:"
    echo "  docker-compose up -d postgres"
    exit 1
fi
echo "✅ PostgreSQL is running"

# Create test database if it doesn't exist
echo "Creating bsky_test database..."
psql -h localhost -U postgres -tc "SELECT 1 FROM pg_database WHERE datname = 'bsky_test'" | grep -q 1 || \
    psql -h localhost -U postgres -c "CREATE DATABASE bsky_test;"
echo "✅ Database bsky_test created/exists"

# Clear any existing data
echo "Clearing Redis streams..."
redis-cli DEL firehose_live firehose_backfill repo_backfill label_live > /dev/null 2>&1 || true
echo "✅ Redis streams cleared"

echo ""
echo "✅ Setup complete! You can now run:"
echo "   cd .."
echo "   cargo test -p rsky-integration-tests -- --ignored --nocapture"
