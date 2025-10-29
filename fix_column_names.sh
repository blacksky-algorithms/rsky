#!/bin/bash
# Convert all camelCase column names to snake_case in plugin files

PLUGINS_DIR="rsky-indexer/src/indexing/plugins"

# Function to convert camelCase to snake_case in SQL column names
fix_file() {
    local file="$1"
    echo "Fixing $file..."
    
    # Create temp file
    tmp_file=$(mktemp)
    
    # Apply sed transformations for all camelCase patterns in SQL queries
    sed -E '
        s/"subjectDid"/subject_did/g
        s/"subjectCid"/subject_cid/g
        s/"createdAt"/created_at/g
        s/"indexedAt"/indexed_at/g
        s/"sortAt"/sort_at/g
        s/"replyRoot"/reply_root/g
        s/"replyRootCid"/reply_root_cid/g
        s/"replyParent"/reply_parent/g
        s/"replyParentCid"/reply_parent_cid/g
        s/"postUri"/post_uri/g
        s/"originatorDid"/originator_did/g
        s/"imageCid"/image_cid/g
        s/"videoCid"/video_cid/g
        s/"thumbCid"/thumb_cid/g
        s/"embedUri"/embed_uri/g
        s/"embedCid"/embed_cid/g
        s/"likeCount"/like_count/g
        s/"repostCount"/repost_count/g
        s/"replyCount"/reply_count/g
        s/"quoteCount"/quote_count/g
        s/"followersCount"/followers_count/g
        s/"followsCount"/follows_count/g
        s/"postsCount"/posts_count/g
        s/"listUri"/list_uri/g
        s/"viaCid"/via_cid/g
    ' "$file" > "$tmp_file"
    
    # Replace original file
    mv "$tmp_file" "$file"
}

# Fix all plugin files
for file in "$PLUGINS_DIR"/*.rs; do
    if [ -f "$file" ]; then
        fix_file "$file"
    fi
done

echo "Done! All column names converted to snake_case."
