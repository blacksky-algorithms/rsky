#!/bin/bash
set -eu pipefail

# Get GitHub event variables from environment
EVENT_NAME="${GITHUB_EVENT_NAME}"
PR_BASE_SHA="${PR_BASE_SHA:-}"
PR_HEAD_SHA="${PR_HEAD_SHA:-}"
GITHUB_OUTPUT="${GITHUB_OUTPUT:-/dev/stdout}"

# Check if we're on the base repository or a fork
IS_FORK=false
if [[ "$EVENT_NAME" == "pull_request" ]]; then
    PR_HEAD_REPO="${PR_HEAD_REPO:-$GITHUB_REPOSITORY}"
    if [[ "$PR_HEAD_REPO" != "blacksky-algorithms/rsky" ]]; then
        IS_FORK=true
    fi
fi

echo "Is fork repository: $IS_FORK"

# Define the expected dockerfile paths
DOCKERFILE_DIRS=("rsky-firehose" "rsky-jetstream-subscriber" "rsky-pds")

# Check which Dockerfiles have changed
CHANGED_DOCKERFILES=()

if [[ "$EVENT_NAME" == "pull_request" ]]; then
    BASE_SHA=$(git merge-base "$PR_BASE_SHA" "$PR_HEAD_SHA")
    DIFF_FILES=$(git diff --name-only $BASE_SHA "$PR_HEAD_SHA")
else
    # For push events, compare with the previous commit
    DIFF_FILES=$(git diff --name-only HEAD^ HEAD)
fi

echo "Changed files:"
echo "$DIFF_FILES"

# Check each Dockerfile path
for dir in "${DOCKERFILE_DIRS[@]}"; do
    if echo "$DIFF_FILES" | grep -q "^$dir/"; then
        # Check if Dockerfile exists
        if [[ -f "$dir/Dockerfile" ]]; then
            CHANGED_DOCKERFILES+=("$dir")
            echo "Dockerfile with changes: $dir"
        fi
    fi
done

# Handle case where no Dockerfiles have changes
if [ ${#CHANGED_DOCKERFILES[@]} -eq 0 ]; then
    echo "No Dockerfiles with changes found"
    echo "dockerfiles=[]" >> "$GITHUB_OUTPUT"
    echo "is_fork=$IS_FORK" >> "$GITHUB_OUTPUT"
    echo "No Dockerfiles to process"
    exit 0
fi

# Convert to JSON array for matrix
JSON_DOCKERFILES=$(printf '%s\n' "${CHANGED_DOCKERFILES[@]}" | jq -R -s -c 'split("\n") | map(select(length > 0))')
echo "dockerfiles=$JSON_DOCKERFILES" >> "$GITHUB_OUTPUT"
echo "is_fork=$IS_FORK" >> "$GITHUB_OUTPUT"
echo "Found Dockerfiles with changes: $JSON_DOCKERFILES"