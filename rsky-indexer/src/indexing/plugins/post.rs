use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;
use tracing::debug;

pub struct PostPlugin;

impl PostPlugin {
    /// Extract creator DID from AT URI (format: at://did:plc:xyz/collection/rkey)
    fn extract_creator(uri: &str) -> Option<String> {
        if let Some(stripped) = uri.strip_prefix("at://") {
            if let Some(did_end) = stripped.find('/') {
                return Some(stripped[..did_end].to_string());
            }
        }
        None
    }

    /// Extract facets from record (mentions and links)
    fn extract_facets(record: &JsonValue) -> (Vec<String>, Vec<String>) {
        let mut mentions = Vec::new();
        let mut links = Vec::new();

        if let Some(facets) = record.get("facets").and_then(|f| f.as_array()) {
            for facet in facets {
                if let Some(features) = facet.get("features").and_then(|f| f.as_array()) {
                    for feature in features {
                        if let Some(type_str) = feature.get("$type").and_then(|t| t.as_str()) {
                            if type_str == "app.bsky.richtext.facet#mention" {
                                if let Some(did) = feature.get("did").and_then(|d| d.as_str()) {
                                    mentions.push(did.to_string());
                                }
                            } else if type_str == "app.bsky.richtext.facet#link" {
                                if let Some(uri) = feature.get("uri").and_then(|u| u.as_str()) {
                                    links.push(uri.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        (mentions, links)
    }

    /// Process image embeds
    async fn process_image_embeds(
        client: &deadpool_postgres::Object,
        post_uri: &str,
        embed: &JsonValue,
    ) -> Result<(), IndexerError> {
        if let Some(images) = embed.get("images").and_then(|i| i.as_array()) {
            for (position, image) in images.iter().enumerate() {
                let image_cid = image
                    .get("image")
                    .and_then(|i| i.get("ref"))
                    .and_then(|r| r.as_str());
                let alt = image.get("alt").and_then(|a| a.as_str()).unwrap_or("");

                if let Some(cid) = image_cid {
                    client
                        .execute(
                            r#"INSERT INTO post_embed_image ("postUri", position, "imageCid", alt)
                               VALUES ($1, $2, $3, $4)
                               ON CONFLICT ("postUri", position) DO NOTHING"#,
                            &[&post_uri, &(position as i32), &cid, &alt],
                        )
                        .await
                        .map_err(|e| IndexerError::Database(e.into()))?;
                }
            }
        }
        Ok(())
    }

    /// Process external embed
    async fn process_external_embed(
        client: &deadpool_postgres::Object,
        post_uri: &str,
        embed: &JsonValue,
    ) -> Result<(), IndexerError> {
        if let Some(external) = embed.get("external") {
            let uri = external.get("uri").and_then(|u| u.as_str());
            let title = external.get("title").and_then(|t| t.as_str()).unwrap_or("");
            let description = external.get("description").and_then(|d| d.as_str()).unwrap_or("");
            let thumb_cid = external
                .get("thumb")
                .and_then(|t| t.get("ref"))
                .and_then(|r| r.as_str());

            if let Some(ext_uri) = uri {
                client
                    .execute(
                        r#"INSERT INTO post_embed_external ("postUri", uri, title, description, "thumbCid")
                           VALUES ($1, $2, $3, $4, $5)
                           ON CONFLICT (post_uri) DO NOTHING"#,
                        &[&post_uri, &ext_uri, &title, &description, &thumb_cid],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;
            }
        }
        Ok(())
    }

    /// Process video embed
    async fn process_video_embed(
        client: &deadpool_postgres::Object,
        post_uri: &str,
        embed: &JsonValue,
    ) -> Result<(), IndexerError> {
        let video_cid = embed
            .get("video")
            .and_then(|v| v.get("ref"))
            .and_then(|r| r.as_str());
        let alt = embed.get("alt").and_then(|a| a.as_str()).unwrap_or("");

        if let Some(cid) = video_cid {
            client
                .execute(
                    r#"INSERT INTO post_embed_video ("postUri", "videoCid", alt)
                       VALUES ($1, $2, $3)
                       ON CONFLICT (post_uri) DO NOTHING"#,
                    &[&post_uri, &cid, &alt],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }
        Ok(())
    }

    /// Process record embed (quote)
    async fn process_record_embed(
        client: &deadpool_postgres::Object,
        post_uri: &str,
        post_cid: &str,
        creator: &str,
        embed: &JsonValue,
        indexed_at: &DateTime<Utc>,
    ) -> Result<Option<String>, IndexerError> {
        if let Some(record) = embed.get("record") {
            let record_uri = record.get("uri").and_then(|u| u.as_str());
            let record_cid = record.get("cid").and_then(|c| c.as_str());

            if let (Some(subject_uri), Some(subject_cid)) = (record_uri, record_cid) {
                // Insert into post_embed_record
                client
                    .execute(
                        r#"INSERT INTO post_embed_record ("postUri", "embedUri", "embedCid")
                           VALUES ($1, $2, $3)
                           ON CONFLICT (post_uri) DO NOTHING"#,
                        &[&post_uri, &subject_uri, &subject_cid],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;

                // Insert into quote table
                client
                    .execute(
                        r#"INSERT INTO quote (uri, cid, creator, subject, "subjectCid", "createdAt", "indexedAt")
                           VALUES ($1, $2, $3, $4, $5, $6, $7)
                           ON CONFLICT (uri) DO NOTHING"#,
                        &[&post_uri, &post_cid, &creator, &subject_uri, &subject_cid, &indexed_at.to_rfc3339(), &indexed_at.to_rfc3339()],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;

                return Ok(Some(subject_uri.to_string()));
            }
        }
        Ok(None)
    }

    /// Parse ISO8601/RFC3339 timestamp string to DateTime<Utc>
    fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, IndexerError> {
        DateTime::parse_from_rfc3339(timestamp)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| IndexerError::Serialization(format!("Invalid timestamp '{}': {}", timestamp, e)))
    }

    /// Hash a string to i64 for PostgreSQL advisory lock
    /// Uses a simple hash function similar to Java's hashCode
    fn hash_lock_key(key: &str) -> i64 {
        let mut hash: i64 = 0;
        for byte in key.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as i64);
        }
        hash
    }

    /// Execute aggregate update with coalescing lock to avoid thrashing during backfills
    /// Matches TypeScript's coalesceWithLock pattern
    /// If lock cannot be acquired, skip the update (another transaction is handling it)
    async fn update_with_coalesce_lock(
        pool: &Pool,
        lock_key: &str,
        did: &str,
        query: &str,
    ) -> Result<(), IndexerError> {
        // Get a connection from the pool
        let mut client = pool.get().await?;

        // Begin transaction
        let txn = client
            .transaction()
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Try to acquire advisory lock (auto-released at transaction end)
        let lock_id = Self::hash_lock_key(lock_key);
        let lock_acquired: bool = txn
            .query_one("SELECT pg_try_advisory_xact_lock($1)", &[&lock_id])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?
            .get(0);

        if lock_acquired {
            // Lock acquired, perform the update
            txn.execute(query, &[&did])
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            // Commit transaction (releases lock)
            txn.commit()
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        } else {
            // Lock not acquired, another transaction is handling it
            // Rollback and skip (coalescing behavior)
            txn.rollback()
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
    }

    /// Get reply ancestors up to depth 5
    /// Returns Vec<(uri, creator, height)> where height is distance from current post
    async fn get_reply_ancestors(
        client: &deadpool_postgres::Object,
        parent_uri: &str,
        max_depth: i32,
    ) -> Result<Vec<(String, String, i32)>, IndexerError> {
        let mut ancestors = Vec::new();
        let mut current_uri = parent_uri.to_string();
        let mut height = 0;

        while height < max_depth {
            // Get the post and its creator
            let row = client
                .query_opt(
                    r#"SELECT creator, "replyParent" FROM post WHERE uri = $1"#,
                    &[&current_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            match row {
                Some(r) => {
                    let creator: Option<String> = r.get(0);
                    let reply_parent: Option<String> = r.get(1);

                    if let Some(ancestor_creator) = creator {
                        ancestors.push((current_uri.clone(), ancestor_creator, height));
                    }

                    // Move to the next parent
                    match reply_parent {
                        Some(parent) => {
                            current_uri = parent;
                            height += 1;
                        }
                        None => break,
                    }
                }
                None => break,
            }
        }

        Ok(ancestors)
    }

    /// Get descendants of a post (replies that already exist)
    /// Used for out-of-order indexing - when a parent is indexed after its children
    /// Returns Vec<(uri, creator, cid, sort_at, depth)> where depth is distance from current post
    async fn get_descendants(
        client: &deadpool_postgres::Object,
        post_uri: &str,
        max_depth: i32,
    ) -> Result<Vec<(String, String, String, DateTime<Utc>, i32)>, IndexerError> {
        // Recursive CTE to find all descendants up to max_depth
        let rows = client
            .query(
                r#"
                WITH RECURSIVE descendent(uri, creator, cid, sort_at, depth) AS (
                    -- Base case: direct replies to this post
                    SELECT uri, creator, cid, "sortAt", 1 as depth
                    FROM post
                    WHERE "replyParent" = $1
                    AND $2 >= 1

                    UNION ALL

                    -- Recursive case: replies to descendants
                    SELECT p.uri, p.creator, p.cid, p."sortAt", d.depth + 1
                    FROM post p
                    INNER JOIN descendent d ON d.uri = p."replyParent"
                    WHERE d.depth < $2
                )
                SELECT uri, creator, cid, "sortAt", depth
                FROM descendent
                ORDER BY depth, sort_at
                "#,
                &[&post_uri, &max_depth],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let mut descendants = Vec::new();
        for row in rows {
            let uri: String = row.get(0);
            let creator: String = row.get(1);
            let cid: String = row.get(2);
            let sort_at: DateTime<Utc> = row.get(3);
            let depth: i32 = row.get(4);
            descendants.push((uri, creator, cid, sort_at, depth));
        }

        Ok(descendants)
    }

    /// Check if reply root is invalid
    /// Matches TypeScript invalidReplyRoot logic:
    /// - If parent has invalidReplyRoot set, transitively this is invalid too
    /// - If replying to root post directly, ensure the root doesn't have a reply field
    /// - If replying to a reply, ensure parent's reply.root matches this reply's root
    async fn check_invalid_reply_root(
        client: &deadpool_postgres::Object,
        reply_root_uri: &str,
        reply_parent_uri: &str,
    ) -> Result<bool, IndexerError> {
        // Get parent post and its reply info
        let parent_row = client
            .query_opt(
                r#"SELECT "invalidReplyRoot", "replyRoot" FROM post WHERE uri = $1"#,
                &[&reply_parent_uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let Some(parent) = parent_row else {
            // Parent doesn't exist, invalid reply
            return Ok(true);
        };

        let parent_invalid: Option<bool> = parent.get(0);
        let parent_reply_root: Option<String> = parent.get(1);

        // If parent is invalid, transitively this is invalid
        if parent_invalid == Some(true) {
            return Ok(true);
        }

        // If replying directly to root (parent == root)
        if reply_parent_uri == reply_root_uri {
            // Root post should not itself be a reply
            return Ok(parent_reply_root.is_some());
        }

        // If replying to a reply, ensure parent's root matches our root
        Ok(parent_reply_root.as_deref() != Some(reply_root_uri))
    }

    /// Check if reply violates threadgate rules
    /// Matches TypeScript violatesThreadGate logic:
    /// Checks if replier is allowed based on threadgate rules (mention, following, followers, lists)
    async fn check_violates_threadgate(
        client: &deadpool_postgres::Object,
        replier_did: &str,
        root_post_uri: &str,
    ) -> Result<bool, IndexerError> {
        // Get root post creator
        let root_creator = Self::extract_creator(root_post_uri);
        let Some(owner_did) = root_creator else {
            return Ok(false); // Can't validate without owner
        };

        // Owner can always reply to their own threads
        if replier_did == owner_did {
            return Ok(false);
        }

        // Get threadgate record for root post
        // ThreadGate URI format: at://did/app.bsky.feed.threadgate/rkey
        let root_rkey = root_post_uri.rsplit('/').next();
        let Some(rkey) = root_rkey else {
            return Ok(false);
        };

        let threadgate_uri = format!("at://{}/app.bsky.feed.threadgate/{}", owner_did, rkey);

        // Query threadgate record from record table
        let threadgate_row = client
            .query_opt(
                "SELECT json FROM record WHERE uri = $1 AND json != '{}'",
                &[&threadgate_uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let Some(row) = threadgate_row else {
            // No threadgate, anyone can reply
            return Ok(false);
        };

        let json: JsonValue = row.get(0);

        // Parse threadgate allow rules
        let allow_array = json
            .get("allow")
            .and_then(|a| a.as_array());

        let Some(rules) = allow_array else {
            // No allow rules means anyone can reply
            return Ok(false);
        };

        if rules.is_empty() {
            // Empty allow list means only owner can reply
            return Ok(true);
        }

        // Check each rule
        for rule in rules {
            if let Some(rule_type) = rule.get("$type").and_then(|t| t.as_str()) {
                match rule_type {
                    "app.bsky.feed.threadgate#mentionRule" => {
                        // Check if root post mentions the replier
                        let root_post = client
                            .query_opt(
                                "SELECT json FROM record WHERE uri = $1",
                                &[&root_post_uri],
                            )
                            .await
                            .map_err(|e| IndexerError::Database(e.into()))?;

                        if let Some(post_row) = root_post {
                            let post_json: JsonValue = post_row.get(0);
                            let (mentions, _) = Self::extract_facets(&post_json);
                            if mentions.contains(&replier_did.to_string()) {
                                return Ok(false); // Mentioned in root post, allowed
                            }
                        }
                    }
                    "app.bsky.feed.threadgate#followingRule" => {
                        // Check if owner follows replier
                        let follows = client
                            .query_opt(
                                r#"SELECT uri FROM follow WHERE creator = $1 AND "subjectDid" = $2"#,
                                &[&owner_did, &replier_did],
                            )
                            .await
                            .map_err(|e| IndexerError::Database(e.into()))?;

                        if follows.is_some() {
                            return Ok(false); // Owner follows replier, allowed
                        }
                    }
                    "app.bsky.feed.threadgate#listRule" => {
                        // Check if replier is in the specified list
                        if let Some(list_uri) = rule.get("list").and_then(|l| l.as_str()) {
                            let in_list = client
                                .query_opt(
                                    r#"SELECT uri FROM list_item WHERE "listUri" = $1 AND subject_did = $2"#,
                                    &[&list_uri, &replier_did],
                                )
                                .await
                                .map_err(|e| IndexerError::Database(e.into()))?;

                            if in_list.is_some() {
                                return Ok(false); // In allowed list, allowed
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // No rules matched, violates threadgate
        Ok(true)
    }

    /// Check if quote embed violates postgate embedding rules
    /// Matches TypeScript validatePostEmbed logic:
    /// Checks if quoted post has postgate that disallows embedding
    async fn check_violates_embedding_rules(
        client: &deadpool_postgres::Object,
        embed_uri: &str,
        quoter_did: &str,
    ) -> Result<bool, IndexerError> {
        // Get quoted post creator
        let embed_creator = Self::extract_creator(embed_uri);
        let Some(quoted_author) = embed_creator else {
            return Ok(false); // Can't validate without author
        };

        // Author can always quote their own posts
        if quoter_did == quoted_author {
            return Ok(false);
        }

        // Get postgate record for quoted post
        // PostGate URI format: at://did/app.bsky.feed.postgate/rkey
        let embed_rkey = embed_uri.rsplit('/').next();
        let Some(rkey) = embed_rkey else {
            return Ok(false);
        };

        let postgate_uri = format!("at://{}/app.bsky.feed.postgate/{}", quoted_author, rkey);

        // Query postgate record from record table
        let postgate_row = client
            .query_opt(
                "SELECT json FROM record WHERE uri = $1 AND json != '{}'",
                &[&postgate_uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let Some(row) = postgate_row else {
            // No postgate, embedding is allowed
            return Ok(false);
        };

        let json: JsonValue = row.get(0);

        // Check embeddingRules
        let embedding_rules = json.get("embeddingRules");

        // If embeddingRules is not present or is an empty array, embedding is allowed
        let Some(rules_array) = embedding_rules.and_then(|r| r.as_array()) else {
            return Ok(false);
        };

        if rules_array.is_empty() {
            return Ok(false);
        }

        // Check each rule
        for rule in rules_array {
            if let Some(rule_type) = rule.get("$type").and_then(|t| t.as_str()) {
                if rule_type == "app.bsky.feed.postgate#disableRule" {
                    // Embedding is explicitly disabled
                    return Ok(true);
                }
            }
        }

        // No disable rule found, embedding is allowed
        Ok(false)
    }
}

#[async_trait]
impl RecordPlugin for PostPlugin {
    fn collection(&self) -> &str {
        "app.bsky.feed.post"
    }

    async fn insert(
        &self,
        pool: &Pool,
        uri: &str,
        cid: &str,
        record: &JsonValue,
        timestamp: &str,
    ) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        // Extract creator from URI
        let creator = Self::extract_creator(uri);

        // Extract core fields
        let text = record.get("text").and_then(|v| v.as_str()).unwrap_or("");

        // Parse timestamps
        let indexed_at = Self::parse_timestamp(timestamp)?;
        let created_at = match record.get("createdAt").and_then(|c| c.as_str()) {
            Some(ts) => Self::parse_timestamp(ts)?,
            None => indexed_at.clone(),
        };

        // Extract reply fields
        let reply_root = record
            .get("reply")
            .and_then(|r| r.get("root"))
            .and_then(|r| r.get("uri"))
            .and_then(|u| u.as_str());
        let reply_root_cid = record
            .get("reply")
            .and_then(|r| r.get("root"))
            .and_then(|r| r.get("cid"))
            .and_then(|c| c.as_str());
        let reply_parent = record
            .get("reply")
            .and_then(|r| r.get("parent"))
            .and_then(|p| p.get("uri"))
            .and_then(|u| u.as_str());
        let reply_parent_cid = record
            .get("reply")
            .and_then(|r| r.get("parent"))
            .and_then(|p| p.get("cid"))
            .and_then(|c| c.as_str());

        // Extract langs and tags as Vec<String> for TEXT[] columns
        let langs: Option<Vec<String>> = record
            .get("langs")
            .and_then(|l| l.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        let tags: Option<Vec<String>> = record
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

        // Calculate sortAt (MIN(indexedAt, createdAt)) for feed_item and notifications
        let sort_at = if created_at < indexed_at {
            created_at.clone()
        } else {
            indexed_at.clone()
        };

        // Insert post
        let rows_inserted = client
            .execute(
                r#"INSERT INTO post (uri, cid, creator, text, "replyRoot", "replyRootCid", "replyParent", "replyParentCid",
                                     langs, tags, "createdAt", "indexedAt")
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[
                    &uri,
                    &cid,
                    &creator,
                    &text,
                    &reply_root,
                    &reply_root_cid,
                    &reply_parent,
                    &reply_parent_cid,
                    &langs,
                    &tags,
                    &created_at.to_rfc3339(),
                    &indexed_at.to_rfc3339(),
                ],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        if rows_inserted == 0 {
            // Post already indexed, skip
            return Ok(());
        }

        // Insert into feed_item
        client
            .execute(
                r#"INSERT INTO feed_item (type, uri, cid, "postUri", "originatorDid", "sortAt")
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (uri, cid) DO NOTHING"#,
                &[&"post", &uri, &cid, &uri, &creator, &sort_at.to_rfc3339()],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Extract facets (mentions and links)
        let (mentions, _links) = Self::extract_facets(record);

        // Create notifications for mentions (prevent self-notifications)
        if let Some(post_creator) = &creator {
            for mentioned_did in mentions {
                if &mentioned_did != post_creator {
                    client
                        .execute(
                            r#"INSERT INTO notification (did, author, "recordUri", "recordCid", reason, "reasonSubject", "sortAt")
                               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                            &[&mentioned_did, &post_creator, &uri, &cid, &"mention", &Some(uri), &sort_at.to_rfc3339()],
                        )
                        .await
                        .map_err(|e| IndexerError::Database(e.into()))?;
                }
            }
        }

        // Validate reply if this is a reply
        let (_invalid_reply_root, violates_threadgate) = if let (Some(root_uri), Some(parent_uri), Some(post_creator)) = (reply_root, reply_parent, &creator) {
            // Check if reply root is invalid
            let invalid = Self::check_invalid_reply_root(&client, root_uri, parent_uri).await?;

            // Check if reply violates threadgate rules
            let violates = if !invalid {
                Self::check_violates_threadgate(&client, post_creator, root_uri).await?
            } else {
                false
            };

            // Update post with validation flags if any violations
            if invalid || violates {
                client
                    .execute(
                        r#"UPDATE post SET "invalidReplyRoot" = $1, "violatesThreadGate" = $2 WHERE uri = $3"#,
                        &[&invalid, &violates, &uri],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;
            }

            (invalid, violates)
        } else {
            (false, false)
        };

        // Handle embeds
        let mut quote_uri: Option<String> = None;
        if let Some(embed) = record.get("embed") {
            if let Some(embed_type) = embed.get("$type").and_then(|t| t.as_str()) {
                match embed_type {
                    "app.bsky.embed.images" => {
                        Self::process_image_embeds(&client, uri, embed).await?;
                    }
                    "app.bsky.embed.external" => {
                        Self::process_external_embed(&client, uri, embed).await?;
                    }
                    "app.bsky.embed.video" => {
                        Self::process_video_embed(&client, uri, embed).await?;
                    }
                    "app.bsky.embed.record" => {
                        quote_uri = Self::process_record_embed(&client, uri, cid, creator.as_deref().unwrap_or(""), embed, &indexed_at).await?;
                    }
                    "app.bsky.embed.recordWithMedia" => {
                        // Process the record (quote) part
                        quote_uri = Self::process_record_embed(&client, uri, cid, creator.as_deref().unwrap_or(""), embed, &indexed_at).await?;

                        // Process the media part
                        if let Some(media) = embed.get("media") {
                            if let Some(media_type) = media.get("$type").and_then(|t| t.as_str()) {
                                match media_type {
                                    "app.bsky.embed.images" => {
                                        Self::process_image_embeds(&client, uri, media).await?;
                                    }
                                    "app.bsky.embed.external" => {
                                        Self::process_external_embed(&client, uri, media).await?;
                                    }
                                    "app.bsky.embed.video" => {
                                        Self::process_video_embed(&client, uri, media).await?;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Validate quote embed if this post quotes another post
        let mut violates_embedding_rules = false;
        if let (Some(quoted_uri), Some(post_creator)) = (quote_uri.as_ref(), &creator) {
            // Check if quote violates postgate embedding rules
            violates_embedding_rules = Self::check_violates_embedding_rules(&client, quoted_uri, post_creator).await?;

            // Update post with validation flag if violation
            if violates_embedding_rules {
                client
                    .execute(
                        r#"UPDATE post SET "violatesEmbeddingRules" = $1 WHERE uri = $2"#,
                        &[&violates_embedding_rules, &uri],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;
            }
        }

        // Create notification for quote (prevent self-notifications)
        // Don't notify if quote violates embedding rules
        if !violates_embedding_rules {
            if let (Some(quoted_uri), Some(post_creator)) = (quote_uri.as_ref(), &creator) {
                let quoted_creator = Self::extract_creator(quoted_uri);
                if quoted_creator.as_ref() != Some(post_creator) {
                    if let Some(notif_recipient) = quoted_creator {
                        client
                            .execute(
                                r#"INSERT INTO notification (did, author, "recordUri", "recordCid", reason, "reasonSubject", "sortAt")
                                   VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                                &[&notif_recipient, &post_creator, &uri, &cid, &"quote", &Some(quoted_uri.as_str()), &sort_at.to_rfc3339()],
                            )
                            .await
                            .map_err(|e| IndexerError::Database(e.into()))?;
                    }
                }
            }
        }

        // Update aggregates: post_agg.quoteCount for quoted post
        if let Some(quoted_uri) = quote_uri {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, "quoteCount")
                       VALUES ($1, (SELECT COUNT(*) FROM quote WHERE subject = $1))
                       ON CONFLICT (uri) DO UPDATE SET "quoteCount" = EXCLUDED."quoteCount""#,
                    &[&quoted_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Create reply notifications
        // Don't generate reply notifications if post violates threadgate
        if !violates_threadgate {
            if let (Some(parent_uri), Some(post_creator)) = (reply_parent, &creator) {
                // Get ancestors up to depth 5 (parent + 4 more levels)
                let ancestors = Self::get_reply_ancestors(&client, parent_uri, 5).await?;

                // Track which authors we've already notified to avoid duplicates
                let mut notified_authors = std::collections::HashSet::new();

                for (ancestor_uri, ancestor_creator, _height) in ancestors {
                    // Skip if this is the post creator (no self-notifications)
                    if &ancestor_creator == post_creator {
                        continue;
                    }

                    // Skip if we've already notified this author
                    if notified_authors.contains(&ancestor_creator) {
                        continue;
                    }

                    // Create notification
                    client
                        .execute(
                            r#"INSERT INTO notification (did, author, "recordUri", "recordCid", reason, "reasonSubject", "sortAt")
                               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                            &[&ancestor_creator, &post_creator, &uri, &cid, &"reply", &Some(ancestor_uri.as_str()), &sort_at.to_rfc3339()],
                        )
                        .await
                        .map_err(|e| IndexerError::Database(e.into()))?;

                    notified_authors.insert(ancestor_creator);
                }
            }
        }

        // Handle out-of-order indexing: notify about descendants that already exist
        // This happens when a parent post is indexed after its children
        // We need to create notifications for descendants to this post and its ancestors
        const REPLY_NOTIF_DEPTH: i32 = 5;

        // Get descendants of this post (replies that already exist in DB)
        let descendants = Self::get_descendants(&client, uri, REPLY_NOTIF_DEPTH).await?;

        if !descendants.is_empty() {
            // Get ancestors of this post (including self at height 0)
            let mut ancestors_with_self = vec![(uri.to_string(), creator.clone().unwrap_or_default(), 0)];

            // Add ancestors if this is a reply
            if let Some(parent_uri) = reply_parent {
                let parent_ancestors = Self::get_reply_ancestors(&client, parent_uri, REPLY_NOTIF_DEPTH).await?;
                for (ancestor_uri, ancestor_creator, height) in parent_ancestors {
                    // Increment height by 1 since we're going up one more level
                    ancestors_with_self.push((ancestor_uri, ancestor_creator, height + 1));
                }
            }

            // Track which authors we've already notified to avoid duplicates
            let mut notified_authors = std::collections::HashSet::new();

            // For each descendant, check if we should notify ancestors
            for (desc_uri, desc_creator, desc_cid, desc_sort_at, desc_depth) in descendants {
                // For each ancestor of the newly inserted post
                for (ancestor_uri, ancestor_creator, ancestor_height) in &ancestors_with_self {
                    let total_height = desc_depth + ancestor_height;

                    // Only notify if within depth limit
                    if total_height < REPLY_NOTIF_DEPTH {
                        // Skip self-notifications
                        if &desc_creator == ancestor_creator {
                            continue;
                        }

                        // Create a unique key for this notification (recipient + author)
                        let notif_key = format!("{}:{}", ancestor_creator, desc_creator);
                        if notified_authors.contains(&notif_key) {
                            continue;
                        }

                        // Create notification from descendant to ancestor
                        client
                            .execute(
                                r#"INSERT INTO notification (did, author, "recordUri", "recordCid", reason, "reasonSubject", "sortAt")
                                   VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                                &[
                                    ancestor_creator,
                                    &desc_creator,
                                    &desc_uri,
                                    &desc_cid,
                                    &"reply",
                                    &Some(ancestor_uri.as_str()),
                                    &desc_sort_at.to_rfc3339()
                                ],
                            )
                            .await
                            .map_err(|e| IndexerError::Database(e.into()))?;

                        notified_authors.insert(notif_key);
                    }
                }
            }
        }

        // Update aggregates: post_agg.replyCount for parent
        // Only count replies that don't violate threadgate
        if let Some(parent_uri) = reply_parent {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, "replyCount")
                       VALUES ($1, (SELECT COUNT(*) FROM post
                                    WHERE "replyParent" = $1
                                    AND ("violatesThreadGate" IS NULL OR "violatesThreadGate" = false)))
                       ON CONFLICT (uri) DO UPDATE SET "replyCount" = EXCLUDED."replyCount""#,
                    &[&parent_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.postsCount for creator
        // Use explicit locking (coalesceWithLock) to avoid thrash during backfills
        if let Some(post_creator) = &creator {
            let lock_key = format!("postCount:{}", post_creator);
            let query = r#"INSERT INTO profile_agg (did, "postsCount")
                          VALUES ($1, (SELECT COUNT(*) FROM post WHERE creator = $1))
                          ON CONFLICT (did) DO UPDATE SET "postsCount" = EXCLUDED."postsCount""#;

            Self::update_with_coalesce_lock(pool, &lock_key, post_creator, query).await?;
        }

        debug!("Indexed post: {}", uri);
        Ok(())
    }

    async fn update(
        &self,
        _pool: &Pool,
        _uri: &str,
        _cid: &str,
        _record: &JsonValue,
        _timestamp: &str,
    ) -> Result<(), IndexerError> {
        // No-op for post (posts are typically immutable, updates handled via delete+insert)
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        // Get post data before deleting for aggregate updates
        let row = client
            .query_opt(
                r#"SELECT creator, "replyParent" FROM post WHERE uri = $1"#,
                &[&uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let (creator, reply_parent): (Option<String>, Option<String>) = row
            .map(|r| (r.get(0), r.get(1)))
            .unwrap_or((None, None));

        // Get quoted posts before deleting quote records
        let quote_rows = client
            .query("SELECT subject FROM quote WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let quoted_uris: Vec<String> = quote_rows
            .iter()
            .filter_map(|r| r.get(0))
            .collect();

        // Delete post
        client
            .execute("DELETE FROM post WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete from feed_item
        client
            .execute(r#"DELETE FROM feed_item WHERE "postUri" = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete from embed tables
        client
            .execute(r#"DELETE FROM post_embed_image WHERE "postUri" = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute(r#"DELETE FROM post_embed_external WHERE "postUri" = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute(r#"DELETE FROM post_embed_record WHERE "postUri" = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute(r#"DELETE FROM post_embed_video WHERE "postUri" = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete from quote table (both as subject and as quoter)
        client
            .execute(r#"DELETE FROM quote WHERE uri = $1 OR subject = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete notifications
        client
            .execute(r#"DELETE FROM notification WHERE "recordUri" = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Update aggregates: post_agg.replyCount for parent
        // Only count replies that don't violate threadgate
        if let Some(parent_uri) = reply_parent {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, "replyCount")
                       VALUES ($1, (SELECT COUNT(*) FROM post
                                    WHERE "replyParent" = $1
                                    AND ("violatesThreadGate" IS NULL OR "violatesThreadGate" = false)))
                       ON CONFLICT (uri) DO UPDATE SET "replyCount" = EXCLUDED."replyCount""#,
                    &[&parent_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.postsCount for creator
        // Use explicit locking (coalesceWithLock) to avoid thrash during backfills
        if let Some(post_creator) = creator {
            let lock_key = format!("postCount:{}", post_creator);
            let query = r#"INSERT INTO profile_agg (did, "postsCount")
                          VALUES ($1, (SELECT COUNT(*) FROM post WHERE creator = $1))
                          ON CONFLICT (did) DO UPDATE SET "postsCount" = EXCLUDED."postsCount""#;

            Self::update_with_coalesce_lock(pool, &lock_key, &post_creator, query).await?;
        }

        // Update aggregates: post_agg.quoteCount for quoted posts
        for quoted_uri in quoted_uris {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, "quoteCount")
                       VALUES ($1, (SELECT COUNT(*) FROM quote WHERE subject = $1))
                       ON CONFLICT (uri) DO UPDATE SET "quoteCount" = EXCLUDED."quoteCount""#,
                    &[&quoted_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
    }
}
