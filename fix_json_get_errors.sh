#!/bin/bash
# Fix the .get(created_at) errors back to .get("createdAt")

PLUGINS_DIR="rsky-indexer/src/indexing/plugins"

for file in "$PLUGINS_DIR"/*.rs; do
    if [ -f "$file" ]; then
        sed -i '' 's/\.get(created_at)/.get("createdAt")/g' "$file"
    fi
done

echo "Fixed .get() errors"
