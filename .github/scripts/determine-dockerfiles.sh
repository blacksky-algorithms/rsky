#!/bin/bash
set -eu pipefail

# Get GitHub event variables from environment
EVENT_NAME="${GITHUB_EVENT_NAME}"
PR_BASE_SHA="${PR_BASE_SHA:-}"
PR_HEAD_SHA="${PR_HEAD_SHA:-}"
PR_HEAD_REPO="${PR_HEAD_REPO:-}"
GITHUB_OUTPUT="${GITHUB_OUTPUT:-/dev/stdout}"

# Function to convert array to JSON without jq dependency
array_to_json() {
  local array=("$@")
  local json="["
  local separator=""
  
  for item in "${array[@]}"; do
    json="${json}${separator}\"${item}\""
    separator=","
  done
  
  json="${json}]"
  echo "$json"
}

# Determine if this is a fork
IS_FORK="false"
if [[ "$EVENT_NAME" == "pull_request" && -n "$PR_HEAD_REPO" ]]; then
    # If the PR head repo is not the same as the current repo, it's a fork
    if [[ "$PR_HEAD_REPO" != "$GITHUB_REPOSITORY" ]]; then
        IS_FORK="true"
    fi
fi

echo "is_fork=$IS_FORK" >> "$GITHUB_OUTPUT"

# Find all directories with Dockerfiles
ALL_DOCKERFILES=()
while IFS= read -r file; do
    dir=$(dirname "$file")
    # Skip the root Dockerfile if it exists
    if [[ "$dir" != "." ]]; then
        # Remove the leading ./ if present
        dir=${dir#./}
        ALL_DOCKERFILES+=("$dir")
    fi
done < <(find . -name "Dockerfile" -type f | sort)

# Check if .github directory has changes
GITHUB_CHANGES=false
if [[ "$EVENT_NAME" == "pull_request" && -n "$PR_BASE_SHA" && -n "$PR_HEAD_SHA" ]]; then
    BASE_SHA=$(git merge-base "$PR_BASE_SHA" "$PR_HEAD_SHA" || echo "$PR_BASE_SHA")
    if [[ -n "$(git diff --name-only "$BASE_SHA" "$PR_HEAD_SHA" -- .github/ 2>/dev/null || echo '')" ]]; then
        GITHUB_CHANGES=true
    fi
else
    # For push events, compare with the previous commit
    if [[ -n "$(git diff --name-only HEAD^ HEAD -- .github/ 2>/dev/null || echo '')" ]]; then
        GITHUB_CHANGES=true
    fi
fi

echo "GitHub directory changes: $GITHUB_CHANGES"

# Get list of Dockerfiles with changes
CHANGED_DOCKERFILES=()

if [[ "$GITHUB_CHANGES" == "true" ]]; then
    # If .github has changes, include all Dockerfiles
    echo "Changes detected in .github directory, including all Dockerfiles"
    for dockerfile in "${ALL_DOCKERFILES[@]}"; do
        CHANGED_DOCKERFILES+=("$dockerfile")
    done
else
    # Otherwise, only include Dockerfiles with changes
    DIFF_FILES=""
    if [[ "$EVENT_NAME" == "pull_request" && -n "$PR_BASE_SHA" && -n "$PR_HEAD_SHA" ]]; then
        BASE_SHA=$(git merge-base "$PR_BASE_SHA" "$PR_HEAD_SHA" || echo "$PR_BASE_SHA")
        DIFF_FILES=$(git diff --name-only "$BASE_SHA" "$PR_HEAD_SHA" 2>/dev/null || echo '')
    else
        # For push events, compare with the previous commit
        DIFF_FILES=$(git diff --name-only HEAD^ HEAD 2>/dev/null || echo '')
    fi

    echo "Changed files:"
    echo "$DIFF_FILES"

    # Add if the directory or Dockerfile has changed
    for dockerfile in "${ALL_DOCKERFILES[@]}"; do
        if echo "$DIFF_FILES" | grep -q "^$dockerfile/" || echo "$DIFF_FILES" | grep -q "^$dockerfile/Dockerfile"; then
            CHANGED_DOCKERFILES+=("$dockerfile")
            echo "Dockerfile with changes: $dockerfile"
        fi
    done
    
    # Additionally, check if Cargo.toml or Cargo.lock changed
    if echo "$DIFF_FILES" | grep -q "^Cargo\\.\\(toml\\|lock\\)$"; then
        echo "Changes detected in workspace Cargo files, including all Dockerfiles"
        CHANGED_DOCKERFILES=("${ALL_DOCKERFILES[@]}")
    fi
fi

# Convert to JSON array for matrix - without jq dependency
JSON_DOCKERFILES=$(array_to_json "${CHANGED_DOCKERFILES[@]}")
echo "dockerfiles=$JSON_DOCKERFILES" >> "$GITHUB_OUTPUT"
echo "Found Dockerfiles to process: $JSON_DOCKERFILES"
