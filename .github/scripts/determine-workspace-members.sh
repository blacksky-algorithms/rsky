#!/bin/bash
set -eu pipefail

# Get GitHub event variables from environment
EVENT_NAME="${GITHUB_EVENT_NAME}"
PR_BASE_SHA="${PR_BASE_SHA:-}"
PR_HEAD_SHA="${PR_HEAD_SHA:-}"
GITHUB_OUTPUT="${GITHUB_OUTPUT:-/dev/stdout}"

# Extract all packages from Cargo.toml - improved parsing
# This uses a more robust approach to extract package names
WORKSPACE_MEMBERS=$(grep -E 'members\s*=\s*\[' Cargo.toml | 
           sed -e 's/.*\[\s*//' -e 's/\s*\].*//' | 
           grep -o '"[^"]*"' | 
           tr -d '"')

# Define packages to skip
SKIP_PACKAGES=("cypher/frontend" "cypher/backend")

# Check if .github directory has changes
GITHUB_CHANGES=false
if [[ "$EVENT_NAME" == "pull_request" ]]; then
    BASE_SHA=$(git merge-base "$PR_BASE_SHA" "$PR_HEAD_SHA")
    if [[ -n "$(git diff --name-only $BASE_SHA "$PR_HEAD_SHA" -- .github/)" ]]; then
        GITHUB_CHANGES=true
    fi
else
    # For push events, compare with the previous commit
    if [[ -n "$(git diff --name-only HEAD^ HEAD -- .github/)" ]]; then
        GITHUB_CHANGES=true
    fi
fi

echo "GitHub directory changes: $GITHUB_CHANGES"

# Get list of packages with changes
CHANGED_MEMBERS=()

if [[ "$GITHUB_CHANGES" == "true" ]]; then
    # If .github has changes, include all packages (except skipped ones)
    echo "Changes detected in .github directory, including all packages"
    while IFS= read -r pkg; do
        CHANGED_MEMBERS+=("$pkg")
    done <<< "$WORKSPACE_MEMBERS"
else
    # Otherwise, only include packages with changes
    if [[ "$EVENT_NAME" == "pull_request" ]]; then
        BASE_SHA=$(git merge-base "$PR_BASE_SHA" "$PR_HEAD_SHA")
        DIFF_FILES=$(git diff --name-only $BASE_SHA "$PR_HEAD_SHA")
    else
        # For push events, compare with the previous commit
        DIFF_FILES=$(git diff --name-only HEAD^ HEAD)
    fi

    echo "Changed files:"
    echo "$DIFF_FILES"

    while IFS= read -r pkg; do
        if echo "$DIFF_FILES" | grep -q "^$pkg/"; then
            CHANGED_MEMBERS+=("$pkg")
            echo "Package with changes: $pkg"
        fi
    done <<< "$WORKSPACE_MEMBERS"
fi

# Filter out packages to skip
FILTERED_MEMBERS=()
for pkg in "${CHANGED_MEMBERS[@]}"; do
    skip=false
    for skip_pkg in "${SKIP_PACKAGES[@]}"; do
        if [[ "$pkg" == "$skip_pkg" ]]; then
            skip=true
            break
        fi
    done
    if [[ "$skip" == "false" ]]; then
        FILTERED_MEMBERS+=("$pkg")
    fi
done

# Handle case where no packages have changes (avoid empty matrix)
if [ ${#FILTERED_MEMBERS[@]} -eq 0 ]; then
    echo "No workspace members with changes found, defaulting to an empty matrix"
    echo "workspace_members=[]" >> "$GITHUB_OUTPUT"
    echo "No workspace members to process"
    exit 0
fi

# Convert to JSON array for matrix
JSON_MEMBERS=$(printf '%s\n' "${FILTERED_MEMBERS[@]}" | jq -R -s -c 'split("\n") | map(select(length > 0))')
echo "workspace_members=$JSON_MEMBERS" >> "$GITHUB_OUTPUT"
echo "Found workspace members with changes (excluding skipped ones): $JSON_MEMBERS"