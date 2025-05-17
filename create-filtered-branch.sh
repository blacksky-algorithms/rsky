#!/bin/bash
set -e

# Set variables
SOURCE_BRANCH="remove-libipld-and-fix-image-builder"
NEW_BRANCH="feature/remove-libipld-without-workflows"

# Make sure we're on the source branch
current_branch=$(git branch --show-current)
if [ "$current_branch" != "$SOURCE_BRANCH" ]; then
    echo "Error: You must be on the $SOURCE_BRANCH branch to run this script."
    exit 1
fi

# Create the new branch based on the main/master branch
# First, determine default branch (usually main or master)
default_branch=$(git remote show afbase | grep "HEAD branch" | sed 's/.*: //')
echo "Creating new branch $NEW_BRANCH from $default_branch..."
git checkout $default_branch
git pull afbase $default_branch
git checkout -b $NEW_BRANCH

# Get list of changed files in the source branch compared to the default branch
# But exclude .github/workflows files and Dockerfiles in subfolders
changed_files=$(git diff --name-only $default_branch..$SOURCE_BRANCH | grep -v "^\.github/workflows/" | grep -v "/Dockerfile$")

# If there are no changed files, exit
if [ -z "$changed_files" ]; then
    echo "No files to cherry-pick after applying filters."
    exit 1
fi

echo "The following files will be included in the new branch:"
echo "$changed_files"
echo ""

# Save the list of files to a temporary file
temp_file=$(mktemp)
echo "$changed_files" > $temp_file

# For each file, checkout the version from the source branch
echo "Checking out files from $SOURCE_BRANCH..."
while IFS= read -r file; do
    # Make sure the directory exists
    dir=$(dirname "$file")
    mkdir -p "$dir"
    
    # Checkout the file from the source branch
    git checkout $SOURCE_BRANCH -- "$file"
    echo "Added $file"
done < "$temp_file"

# Clean up temporary file
rm $temp_file

# Stage all the files we've checked out
git add .

# Commit the changes
git commit -m "Include changes from $SOURCE_BRANCH except workflows and subdir Dockerfiles"

echo ""
echo "Success! Created branch $NEW_BRANCH with the filtered changes from $SOURCE_BRANCH."
echo "Run 'git push -u origin $NEW_BRANCH' to push the new branch to your remote."
