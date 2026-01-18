//! High-performance bulk loading using `PostgreSQL` `COPY` protocol.
//!
//! `COPY` is significantly faster than `INSERT` for bulk data loading because:
//! - Bypasses SQL parser for data rows
//! - Single transaction for entire batch
//! - Minimal per-row overhead
//!
//! Pattern: `COPY` into temp table, then `INSERT...ON CONFLICT` from temp.

use crate::types::WintermuteError;
use futures::SinkExt;
use futures::pin_mut;
use std::io::Write;

/// Bulk insert records using `COPY` protocol.
/// Returns vector of booleans indicating which records were applied (not stale).
pub async fn copy_insert_records(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, did, json, rev, indexed_at
) -> Result<Vec<bool>, WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(Vec::new());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_record (
                uri text NOT NULL,
                cid text NOT NULL,
                did text NOT NULL,
                json text,
                rev text NOT NULL,
                indexed_at text NOT NULL
            )",
            &[],
        )
        .await?;

    // Truncate in case of reuse
    client.execute("TRUNCATE _bulk_record", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data into temp table
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_record (uri, cid, did, json, rev, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    // Build tab-separated data
    let mut buffer = Vec::with_capacity(data.len() * 200);
    for (uri, cid, did, json, rev, indexed_at) in data {
        // Escape for PostgreSQL COPY text format:
        // - Backslash first (\ -> \\) so we don't double-escape other escapes
        // - Tab (0x09 -> \t)
        // - Newline (0x0a -> \n)
        // - Carriage return (0x0d -> \r)
        let escaped_json = json
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        writeln!(
            buffer,
            "{uri}\t{cid}\t{did}\t{escaped_json}\t{rev}\t{indexed_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    let rows = client
        .query(
            "INSERT INTO record (uri, cid, did, json, rev, \"indexedAt\")
             SELECT uri, cid, did, json, rev, indexed_at
             FROM _bulk_record
             ON CONFLICT (uri) DO UPDATE SET
               rev = EXCLUDED.rev,
               cid = EXCLUDED.cid,
               json = EXCLUDED.json,
               \"indexedAt\" = EXCLUDED.\"indexedAt\"
             WHERE record.rev <= EXCLUDED.rev
             RETURNING uri",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW record bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            count
        );
    }

    let applied_uris: std::collections::HashSet<String> =
        rows.iter().map(|r| r.get::<_, String>(0)).collect();

    Ok(data
        .iter()
        .map(|(uri, ..)| applied_uris.contains(uri))
        .collect())
}

/// Bulk insert actors using `COPY` protocol.
pub async fn copy_ensure_actors(
    client: &deadpool_postgres::Client,
    dids: &[&str],
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if dids.is_empty() {
        return Ok(());
    }

    let count = dids.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_actor (
                did text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_actor", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY dids
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_actor (did) FROM STDIN WITH (FORMAT text)")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(dids.len() * 50);
    for did in dids {
        writeln!(buffer, "{did}")
            .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO actor (did, \"indexedAt\")
             SELECT did, '1970-01-01T00:00:00Z'
             FROM _bulk_actor
             ON CONFLICT (did) DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW actor bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            count
        );
    }

    Ok(())
}

/// Bulk insert posts using `COPY` protocol.
pub async fn copy_insert_posts(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, text, created_at, indexed_at
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_post (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                text text,
                created_at text NOT NULL,
                indexed_at text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_post", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data (no NULL clause - text column is NOT NULL so empty string must be preserved)
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_post (uri, cid, creator, text, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 300);
    for (uri, cid, creator, text, created_at, indexed_at) in data {
        let escaped_text: String = text
            .chars()
            .map(|c| match c {
                '\t' | '\n' | '\r' => ' ',
                _ => c,
            })
            .collect();
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{escaped_text}\t{created_at}\t{indexed_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO post (uri, cid, creator, text, \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, text, created_at, indexed_at
             FROM _bulk_post
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Phase 4: Update profile_agg postsCount for affected creators
    let agg_start = Instant::now();
    client
        .execute(
            "INSERT INTO profile_agg (did, \"postsCount\")
             SELECT creator, COUNT(*) FROM post
             WHERE creator IN (SELECT DISTINCT creator FROM _bulk_post)
             GROUP BY creator
             ON CONFLICT (did) DO UPDATE SET \"postsCount\" = EXCLUDED.\"postsCount\"",
            &[],
        )
        .await?;
    let agg_ms = agg_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms + agg_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW post bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms, agg={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            agg_ms,
            count
        );
    }

    Ok(())
}

/// Bulk insert `feed_item` records using `COPY` protocol.
pub async fn copy_insert_feed_items(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // type, uri, cid, post_uri, originator_did, sort_at
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_feed_item (
                type text NOT NULL,
                uri text NOT NULL,
                cid text NOT NULL,
                post_uri text NOT NULL,
                originator_did text NOT NULL,
                sort_at text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_feed_item", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_feed_item (type, uri, cid, post_uri, originator_did, sort_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 200);
    for (item_type, uri, cid, post_uri, originator_did, sort_at) in data {
        writeln!(
            buffer,
            "{item_type}\t{uri}\t{cid}\t{post_uri}\t{originator_did}\t{sort_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO feed_item (type, uri, cid, \"postUri\", \"originatorDid\", \"sortAt\")
             SELECT type, uri, cid, post_uri, originator_did, sort_at
             FROM _bulk_feed_item
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW feed_item bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            count
        );
    }

    Ok(())
}

/// Bulk insert likes using `COPY` protocol.
pub async fn copy_insert_likes(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String, String)], // uri, cid, creator, subject, subject_cid, created_at, indexed_at
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_like (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                subject text NOT NULL,
                subject_cid text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_like", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_like (uri, cid, creator, subject, subject_cid, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 250);
    for (uri, cid, creator, subject, subject_cid, created_at, indexed_at) in data {
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{subject}\t{subject_cid}\t{created_at}\t{indexed_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO \"like\" (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, subject_cid, created_at, indexed_at
             FROM _bulk_like
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW like bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            count
        );
    }

    Ok(())
}

/// Bulk insert follows using `COPY` protocol.
pub async fn copy_insert_follows(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, subject_did, created_at, indexed_at
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_follow (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                subject_did text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_follow", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_follow (uri, cid, creator, subject_did, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 200);
    for (uri, cid, creator, subject_did, created_at, indexed_at) in data {
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{subject_did}\t{created_at}\t{indexed_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO follow (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject_did, created_at, indexed_at
             FROM _bulk_follow
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Phase 4: Update profile_agg followsCount and followersCount
    let agg_start = Instant::now();
    // Update followsCount for creators (those who are following)
    client
        .execute(
            "INSERT INTO profile_agg (did, \"followsCount\")
             SELECT creator, COUNT(*) FROM follow
             WHERE creator IN (SELECT DISTINCT creator FROM _bulk_follow)
             GROUP BY creator
             ON CONFLICT (did) DO UPDATE SET \"followsCount\" = EXCLUDED.\"followsCount\"",
            &[],
        )
        .await?;
    // Update followersCount for subjects (those who are followed)
    client
        .execute(
            "INSERT INTO profile_agg (did, \"followersCount\")
             SELECT \"subjectDid\", COUNT(*) FROM follow
             WHERE \"subjectDid\" IN (SELECT DISTINCT subject_did FROM _bulk_follow)
             GROUP BY \"subjectDid\"
             ON CONFLICT (did) DO UPDATE SET \"followersCount\" = EXCLUDED.\"followersCount\"",
            &[],
        )
        .await?;
    let agg_ms = agg_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms + agg_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW follow bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms, agg={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            agg_ms,
            count
        );
    }

    Ok(())
}

/// Bulk insert reposts using `COPY` protocol.
pub async fn copy_insert_reposts(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String, String)], // uri, cid, creator, subject, subject_cid, created_at, indexed_at
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_repost (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                subject text NOT NULL,
                subject_cid text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_repost", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_repost (uri, cid, creator, subject, subject_cid, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 250);
    for (uri, cid, creator, subject, subject_cid, created_at, indexed_at) in data {
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{subject}\t{subject_cid}\t{created_at}\t{indexed_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO repost (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, subject_cid, created_at, indexed_at
             FROM _bulk_repost
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW repost bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            count
        );
    }

    Ok(())
}

/// Bulk insert blocks using `COPY` protocol.
pub async fn copy_insert_blocks(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, subject, created_at, indexed_at
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_block (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                subject text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_block", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_block (uri, cid, creator, subject, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 200);
    for (uri, cid, creator, subject, created_at, indexed_at) in data {
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{subject}\t{created_at}\t{indexed_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO actor_block (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, created_at, indexed_at
             FROM _bulk_block
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW block bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            count
        );
    }

    Ok(())
}

/// Bulk insert `post_embed_image` records using `COPY` protocol.
pub async fn copy_insert_post_embed_images(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String)], // post_uri, position, image_cid, alt
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_post_embed_image (
                post_uri text NOT NULL,
                position text NOT NULL,
                image_cid text NOT NULL,
                alt text NOT NULL
            )",
            &[],
        )
        .await?;

    client
        .execute("TRUNCATE _bulk_post_embed_image", &[])
        .await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_post_embed_image (post_uri, position, image_cid, alt) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 150);
    for (post_uri, position, image_cid, alt) in data {
        // Escape alt text for tabs/newlines
        let escaped_alt: String = alt
            .chars()
            .map(|c| match c {
                '\t' | '\n' | '\r' => ' ',
                _ => c,
            })
            .collect();
        writeln!(buffer, "{post_uri}\t{position}\t{image_cid}\t{escaped_alt}")
            .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO post_embed_image (\"postUri\", position, \"imageCid\", alt)
             SELECT post_uri, position, image_cid, alt
             FROM _bulk_post_embed_image
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW post_embed_image bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            count
        );
    }

    Ok(())
}

/// Bulk insert `post_embed_video` records using `COPY` protocol.
pub async fn copy_insert_post_embed_videos(
    client: &deadpool_postgres::Client,
    data: &[(String, String, Option<String>)], // post_uri, video_cid, alt
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_post_embed_video (
                post_uri text NOT NULL,
                video_cid text NOT NULL,
                alt text
            )",
            &[],
        )
        .await?;

    client
        .execute("TRUNCATE _bulk_post_embed_video", &[])
        .await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data (with NULL handling for alt)
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_post_embed_video (post_uri, video_cid, alt) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '\\N')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 150);
    for (post_uri, video_cid, alt) in data {
        let escaped_alt = match alt {
            Some(a) => a
                .chars()
                .map(|c| match c {
                    '\t' | '\n' | '\r' => ' ',
                    _ => c,
                })
                .collect::<String>(),
            None => "\\N".to_owned(), // PostgreSQL NULL marker
        };
        writeln!(buffer, "{post_uri}\t{video_cid}\t{escaped_alt}")
            .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO post_embed_video (\"postUri\", \"videoCid\", alt)
             SELECT post_uri, video_cid, alt
             FROM _bulk_post_embed_video
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW post_embed_video bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            count
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_escape_tsv() {
        let text = "hello\tworld\nnewline";
        let escaped = text.replace(['\t', '\n'], " ");
        assert_eq!(escaped, "hello world newline");
    }
}
