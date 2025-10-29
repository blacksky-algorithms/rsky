use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
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
                let image_cid = image.get("image").and_then(|i| i.as_str());
                let alt = image.get("alt").and_then(|a| a.as_str()).unwrap_or("");

                if let Some(cid) = image_cid {
                    client
                        .execute(
                            r#"INSERT INTO post_embed_image (postUri, position, imageCid, alt)
                               VALUES ($1, $2, $3, $4)
                               ON CONFLICT (postUri, position) DO NOTHING"#,
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
            let thumb_cid = external.get("thumb").and_then(|t| t.as_str());

            if let Some(ext_uri) = uri {
                client
                    .execute(
                        r#"INSERT INTO post_embed_external (postUri, uri, title, description, thumbCid)
                           VALUES ($1, $2, $3, $4, $5)
                           ON CONFLICT (postUri) DO NOTHING"#,
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
        let video_cid = embed.get("video").and_then(|v| v.as_str());
        let alt = embed.get("alt").and_then(|a| a.as_str()).unwrap_or("");

        if let Some(cid) = video_cid {
            client
                .execute(
                    r#"INSERT INTO post_embed_video (postUri, videoCid, alt)
                       VALUES ($1, $2, $3)
                       ON CONFLICT (postUri) DO NOTHING"#,
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
        timestamp: &str,
    ) -> Result<Option<String>, IndexerError> {
        if let Some(record) = embed.get("record") {
            let record_uri = record.get("uri").and_then(|u| u.as_str());
            let record_cid = record.get("cid").and_then(|c| c.as_str());

            if let (Some(subject_uri), Some(subject_cid)) = (record_uri, record_cid) {
                // Insert into post_embed_record
                client
                    .execute(
                        r#"INSERT INTO post_embed_record (postUri, embedUri, embedCid)
                           VALUES ($1, $2, $3)
                           ON CONFLICT (postUri) DO NOTHING"#,
                        &[&post_uri, &subject_uri, &subject_cid],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;

                // Insert into quote table
                let created_at = timestamp; // Use indexedAt as createdAt for quote
                client
                    .execute(
                        r#"INSERT INTO quote (uri, cid, creator, subject, subjectCid, createdAt, indexedAt)
                           VALUES ($1, $2, $3, $4, $5, $6, $7)
                           ON CONFLICT (uri) DO NOTHING"#,
                        &[&post_uri, &post_cid, &creator, &subject_uri, &subject_cid, &created_at, &timestamp],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;

                return Ok(Some(subject_uri.to_string()));
            }
        }
        Ok(None)
    }

    /// Get reply ancestors up to depth 5
    async fn get_reply_ancestors(
        client: &deadpool_postgres::Object,
        parent_uri: &str,
        max_depth: i32,
    ) -> Result<Vec<(String, String)>, IndexerError> {
        let mut ancestors = Vec::new();
        let mut current_uri = parent_uri.to_string();
        let mut depth = 0;

        while depth < max_depth {
            // Get the post and its creator
            let row = client
                .query_opt(
                    "SELECT creator, replyParent FROM post WHERE uri = $1",
                    &[&current_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            match row {
                Some(r) => {
                    let creator: Option<String> = r.get(0);
                    let reply_parent: Option<String> = r.get(1);

                    if let Some(ancestor_creator) = creator {
                        ancestors.push((current_uri.clone(), ancestor_creator));
                    }

                    // Move to the next parent
                    match reply_parent {
                        Some(parent) => {
                            current_uri = parent;
                            depth += 1;
                        }
                        None => break,
                    }
                }
                None => break,
            }
        }

        Ok(ancestors)
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
        let created_at = record.get("createdAt").and_then(|c| c.as_str());

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

        // Extract langs and tags as JSON arrays
        let langs = record
            .get("langs")
            .and_then(|l| l.as_array())
            .map(|arr| serde_json::to_string(arr).ok())
            .flatten();
        let tags = record
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| serde_json::to_string(arr).ok())
            .flatten();

        // Calculate sortAt (MIN(indexedAt, createdAt))
        let sort_at = match created_at {
            Some(ca) if ca < timestamp => ca,
            _ => timestamp,
        };

        // Insert post
        let rows_inserted = client
            .execute(
                r#"INSERT INTO post (uri, cid, creator, text, replyRoot, replyRootCid, replyParent, replyParentCid,
                                     langs, tags, createdAt, indexedAt, sortAt)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
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
                    &created_at,
                    &timestamp,
                    &sort_at,
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
                r#"INSERT INTO feed_item (type, uri, cid, postUri, originatorDid, sortAt)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&"post", &uri, &cid, &uri, &creator, &sort_at],
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
                            r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                            &[&mentioned_did, &post_creator, &uri, &cid, &"mention", &Some(uri), &sort_at],
                        )
                        .await
                        .map_err(|e| IndexerError::Database(e.into()))?;
                }
            }
        }

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
                        quote_uri = Self::process_record_embed(&client, uri, cid, creator.as_deref().unwrap_or(""), embed, timestamp).await?;
                    }
                    "app.bsky.embed.recordWithMedia" => {
                        // Process the record (quote) part
                        quote_uri = Self::process_record_embed(&client, uri, cid, creator.as_deref().unwrap_or(""), embed, timestamp).await?;

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

        // Create notification for quote (prevent self-notifications)
        if let (Some(quoted_uri), Some(post_creator)) = (quote_uri.as_ref(), &creator) {
            let quoted_creator = Self::extract_creator(quoted_uri);
            if quoted_creator.as_ref() != Some(post_creator) {
                if let Some(notif_recipient) = quoted_creator {
                    client
                        .execute(
                            r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                            &[&notif_recipient, &post_creator, &uri, &cid, &"quote", &Some(quoted_uri.as_str()), &sort_at],
                        )
                        .await
                        .map_err(|e| IndexerError::Database(e.into()))?;
                }
            }
        }

        // Update aggregates: post_agg.quoteCount for quoted post
        if let Some(quoted_uri) = quote_uri {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, quoteCount)
                       VALUES ($1, (SELECT COUNT(*) FROM quote WHERE subject = $1))
                       ON CONFLICT (uri) DO UPDATE SET quoteCount = EXCLUDED.quoteCount"#,
                    &[&quoted_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Create reply notifications
        if let (Some(parent_uri), Some(post_creator)) = (reply_parent, &creator) {
            // Get ancestors up to depth 5 (parent + 4 more levels)
            let ancestors = Self::get_reply_ancestors(&client, parent_uri, 5).await?;

            // Track which authors we've already notified to avoid duplicates
            let mut notified_authors = std::collections::HashSet::new();

            for (ancestor_uri, ancestor_creator) in ancestors {
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
                        r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                        &[&ancestor_creator, &post_creator, &uri, &cid, &"reply", &Some(ancestor_uri.as_str()), &sort_at],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;

                notified_authors.insert(ancestor_creator);
            }
        }

        // Update aggregates: post_agg.replyCount for parent
        if let Some(parent_uri) = reply_parent {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, replyCount)
                       VALUES ($1, (SELECT COUNT(*) FROM post WHERE replyParent = $1))
                       ON CONFLICT (uri) DO UPDATE SET replyCount = EXCLUDED.replyCount"#,
                    &[&parent_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.postsCount for creator
        if let Some(post_creator) = &creator {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, postsCount)
                       VALUES ($1, (SELECT COUNT(*) FROM post WHERE creator = $1))
                       ON CONFLICT (did) DO UPDATE SET postsCount = EXCLUDED.postsCount"#,
                    &[&post_creator],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // TODO: Validate reply (check invalidReplyRoot, violatesThreadGate)
        // TODO: Validate quote embeds (violatesEmbeddingRules)

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
                "SELECT creator, replyParent FROM post WHERE uri = $1",
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
            .execute("DELETE FROM feed_item WHERE postUri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete from embed tables
        client
            .execute("DELETE FROM post_embed_image WHERE postUri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute("DELETE FROM post_embed_external WHERE postUri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute("DELETE FROM post_embed_record WHERE postUri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute("DELETE FROM post_embed_video WHERE postUri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete from quote table (both as subject and as quoter)
        client
            .execute("DELETE FROM quote WHERE uri = $1 OR subject = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete notifications
        client
            .execute("DELETE FROM notification WHERE record_uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Update aggregates: post_agg.replyCount for parent
        if let Some(parent_uri) = reply_parent {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, replyCount)
                       VALUES ($1, (SELECT COUNT(*) FROM post WHERE replyParent = $1))
                       ON CONFLICT (uri) DO UPDATE SET replyCount = EXCLUDED.replyCount"#,
                    &[&parent_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.postsCount for creator
        if let Some(post_creator) = creator {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, postsCount)
                       VALUES ($1, (SELECT COUNT(*) FROM post WHERE creator = $1))
                       ON CONFLICT (did) DO UPDATE SET postsCount = EXCLUDED.postsCount"#,
                    &[&post_creator],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: post_agg.quoteCount for quoted posts
        for quoted_uri in quoted_uris {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, quoteCount)
                       VALUES ($1, (SELECT COUNT(*) FROM quote WHERE subject = $1))
                       ON CONFLICT (uri) DO UPDATE SET quoteCount = EXCLUDED.quoteCount"#,
                    &[&quoted_uri],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
    }
}
