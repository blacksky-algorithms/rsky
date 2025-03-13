#!/bin/bash
set -eu pipefail

# Get GitHub event variables from environment
EVENT_NAME="${GITHUB_EVENT_NAME}"
PR_BASE_SHA="${PR_BASE_SHA:-}"
PR_HEAD_SHA="${PR_HEAD_SHA:-}"
GITHUB_OUTPUT="${GITHUB_OUTPUT:-/dev/stdout}"

# Extract all packages from Cargo.toml
PACKAGES=$(grep -E 'members\s*=\s*\[' Cargo.toml | sed -e 's/.*\[\s*//' -e 's/\s*\].*//' -e 's/,//g' | tr -d '"' | sed 's/\s\+/\n/g')

# Define packages to skip
SKIP_PACKAGES=("cypher/frontend" "cypher/backend" "rsky-cryptorsky-feedgen")

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
CHANGED_PACKAGES=()

if [[ "$GITHUB_CHANGES" == "true" ]]; then
    # If .github has changes, include all packages (except skipped ones)
    echo "Changes detected in .github directory, including all packages"
    for pkg in $PACKAGES; do
        CHANGED_PACKAGES+=("$pkg")
    done
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

    for pkg in $PACKAGES; do
        if echo "$DIFF_FILES" | grep -q "^$pkg/"; then
            CHANGED_PACKAGES+=("$pkg")
            echo "Package with changes: $pkg"
        fi
    done
fi

# Filter out packages to skip
FILTERED_PACKAGES=()
for pkg in "${CHANGED_PACKAGES[@]}"; do
    skip=false
    for skip_pkg in "${SKIP_PACKAGES[@]}"; do
        if [[ "$pkg" == "$skip_pkg" ]]; then
            skip=true
            break
        fi
    done
    if [[ "$skip" == "false" ]]; then
        FILTERED_PACKAGES+=("$pkg")
    fi
done

# Handle case where no packages have changes (avoid empty matrix)
if [ ${#FILTERED_PACKAGES[@]} -eq 0 ]; then
    echo "No packages with changes found, defaulting to a minimal package"
    # You could set a default minimal package here if needed
    # FILTERED_PACKAGES=("some-default-package")
    echo "packages=[]" >> "$GITHUB_OUTPUT"
    echo "No packages to process"
    exit 0
fi

# Convert to JSON array for matrix
JSON_PACKAGES=$(printf '%s\n' "${FILTERED_PACKAGES[@]}" | jq -R -s -c 'split("\n") | map(select(length > 0))')
echo "packages=$JSON_PACKAGES" >> "$GITHUB_OUTPUT"
echo "Found packages with changes (excluding skipped ones): $JSON_PACKAGES"