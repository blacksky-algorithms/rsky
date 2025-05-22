#!/bin/bash
set -eu pipefail

# Get GitHub event variables from environment
EVENT_NAME="${GITHUB_EVENT_NAME}"
PR_BASE_SHA="${PR_BASE_SHA:-}"
PR_HEAD_SHA="${PR_HEAD_SHA:-}"
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

# Extract all packages from Cargo.toml - improved parsing
echo "Extracting workspace members from Cargo.toml..."
WORKSPACE_MEMBERS=()

# Look for members section in Cargo.toml
if grep -q '\[workspace\]' Cargo.toml; then
  # Extract the members section
  MEMBERS_SECTION=$(sed -n '/\[workspace\]/,/\[/p' Cargo.toml | grep -A 20 'members.*=' | grep -v '^\[')
  
  # Extract member paths - handle both array and table formats
  if echo "$MEMBERS_SECTION" | grep -q 'members.*=.*\['; then
    # Array format: members = ["pkg1", "pkg2"]
    MEMBERS_LIST=$(echo "$MEMBERS_SECTION" | grep -o '"[^"]*"' | tr -d '"')
    readarray -t WORKSPACE_MEMBERS <<< "$MEMBERS_LIST"
  else
    # Fallback: Try to find any directory that contains a Cargo.toml file
    echo "Falling back to finding all directories with Cargo.toml..."
    while IFS= read -r dir; do
      # Skip the root Cargo.toml
      if [[ "$dir" != "./Cargo.toml" ]]; then
        pkg_dir=$(dirname "$dir")
        # Remove the leading ./ if present
        pkg_dir=${pkg_dir#./}
        WORKSPACE_MEMBERS+=("$pkg_dir")
      fi
    done < <(find . -name "Cargo.toml" -type f | sort)
  fi
fi

# If still empty, add some default Rust packages from the directory structure
if [ ${#WORKSPACE_MEMBERS[@]} -eq 0 ]; then
  echo "No workspace members found in Cargo.toml, using detected packages..."
  for dir in $(find . -maxdepth 1 -type d -name "rsky*"); do
    # Remove the leading ./
    dir=${dir#./}
    WORKSPACE_MEMBERS+=("$dir")
  done
fi

echo "Found workspace members: ${WORKSPACE_MEMBERS[*]}"

# Define packages to skip
SKIP_PACKAGES=("cypher/frontend" "cypher/backend" "rsky-pdsadmin")

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

# Get list of packages with changes
CHANGED_MEMBERS=()

if [[ "$GITHUB_CHANGES" == "true" ]]; then
    # If .github has changes, include all packages (except skipped ones)
    echo "Changes detected in .github directory, including all packages"
    for pkg in "${WORKSPACE_MEMBERS[@]}"; do
        CHANGED_MEMBERS+=("$pkg")
    done
else
    # Otherwise, only include packages with changes
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

    for pkg in "${WORKSPACE_MEMBERS[@]}"; do
        if echo "$DIFF_FILES" | grep -q "^$pkg/"; then
            CHANGED_MEMBERS+=("$pkg")
            echo "Package with changes: $pkg"
        fi
    done
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

# Always include at least one default package if array is empty
if [ ${#FILTERED_MEMBERS[@]} -eq 0 ]; then
    echo "No workspace members with changes found, using default fallback"
    # Look for rsky-common as a safe default, or use the first Rust package
    if [[ -d "rsky-common" && -f "rsky-common/Cargo.toml" ]]; then
        FILTERED_MEMBERS+=("rsky-common")
    else
        # Find the first available Rust package
        for dir in "${WORKSPACE_MEMBERS[@]}"; do
            if [[ -d "$dir" && -f "$dir/Cargo.toml" ]]; then
                FILTERED_MEMBERS+=("$dir")
                break
            fi
        done
    fi
    
    # If still empty, use a hardcoded fallback
    if [ ${#FILTERED_MEMBERS[@]} -eq 0 ]; then
        echo "No valid workspace members found, using default package"
        # Use the first 'rsky-' directory as fallback
        for dir in rsky-*; do
            if [[ -d "$dir" && -f "$dir/Cargo.toml" ]]; then
                FILTERED_MEMBERS+=("$dir")
                break
            fi
        done
    fi
fi

# Convert to JSON array for matrix - without jq dependency
JSON_MEMBERS=$(array_to_json "${FILTERED_MEMBERS[@]}")
echo "workspace_members=$JSON_MEMBERS" >> "$GITHUB_OUTPUT"
echo "Found workspace members to process: $JSON_MEMBERS"
