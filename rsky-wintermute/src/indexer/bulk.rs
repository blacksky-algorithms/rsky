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

// Escape a field for COPY text format; borrows when no escaping is needed.
fn escape_copy_field(s: &str) -> std::borrow::Cow<'_, str> {
    if s.bytes().any(|b| matches!(b, b'\\' | b'\t' | b'\n' | b'\r')) {
        std::borrow::Cow::Owned(
            s.replace('\\', "\\\\")
                .replace('\t', "\\t")
                .replace('\n', "\\n")
                .replace('\r', "\\r"),
        )
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

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
        // Strip null bytes which are valid JSON per RFC 8259 but Node.js's
        // JSON.parse() rejects them, causing dataplane rowToRecord parse errors.
        let json = json.replace('\0', "").replace("\\u0000", "");

        // Validate JSON before writing to DB. The record.json column is type text
        // (not jsonb), so PostgreSQL won't reject invalid JSON.
        if serde_json::from_str::<serde_json::Value>(&json).is_err() {
            tracing::error!("bulk insert: skipping {uri} - invalid JSON after serialization");
            continue;
        }

        let uri = escape_copy_field(uri);
        let cid = escape_copy_field(cid);
        let did = escape_copy_field(did);
        let json = escape_copy_field(&json);
        let rev = escape_copy_field(rev);
        let indexed_at = escape_copy_field(indexed_at);
        writeln!(buffer, "{uri}\t{cid}\t{did}\t{json}\t{rev}\t{indexed_at}")
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
        let did = escape_copy_field(did);
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
    compute_agg: bool, // false for the bulk CAR load (aggregates recomputed in one pass after)
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
        let uri = escape_copy_field(uri);
        let cid = escape_copy_field(cid);
        let creator = escape_copy_field(creator);
        let text = escape_copy_field(text);
        let created_at = escape_copy_field(created_at);
        let indexed_at = escape_copy_field(indexed_at);
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{text}\t{created_at}\t{indexed_at}"
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

    // Phase 4: Update profile_agg postsCount for affected creators (skipped during bulk load)
    let agg_start = Instant::now();
    if compute_agg {
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
    }
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
        let item_type = escape_copy_field(item_type);
        let uri = escape_copy_field(uri);
        let cid = escape_copy_field(cid);
        let post_uri = escape_copy_field(post_uri);
        let originator_did = escape_copy_field(originator_did);
        let sort_at = escape_copy_field(sort_at);
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
        let uri = escape_copy_field(uri);
        let cid = escape_copy_field(cid);
        let creator = escape_copy_field(creator);
        let subject = escape_copy_field(subject);
        let subject_cid = escape_copy_field(subject_cid);
        let created_at = escape_copy_field(created_at);
        let indexed_at = escape_copy_field(indexed_at);
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
    compute_agg: bool, // false for the bulk CAR load (aggregates recomputed in one pass after)
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
        let uri = escape_copy_field(uri);
        let cid = escape_copy_field(cid);
        let creator = escape_copy_field(creator);
        let subject_did = escape_copy_field(subject_did);
        let created_at = escape_copy_field(created_at);
        let indexed_at = escape_copy_field(indexed_at);
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

    // Phase 4: Update profile_agg followsCount and followersCount (skipped during bulk load)
    let agg_start = Instant::now();
    if compute_agg {
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
    }
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
        let uri = escape_copy_field(uri);
        let cid = escape_copy_field(cid);
        let creator = escape_copy_field(creator);
        let subject = escape_copy_field(subject);
        let subject_cid = escape_copy_field(subject_cid);
        let created_at = escape_copy_field(created_at);
        let indexed_at = escape_copy_field(indexed_at);
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

/// Bulk insert quotes using `COPY` protocol.
pub async fn copy_insert_quotes(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, subject, subject_cid, created_at, indexed_at
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    let setup_start = Instant::now();
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_quote (
                uri text NOT NULL,
                cid text NOT NULL,
                subject text NOT NULL,
                subject_cid text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_quote", &[]).await?;
    let setup_ms = setup_start.elapsed().as_millis();

    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_quote (uri, cid, subject, subject_cid, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 250);
    for (uri, cid, subject, subject_cid, created_at, indexed_at) in data {
        let uri = escape_copy_field(uri);
        let cid = escape_copy_field(cid);
        let subject = escape_copy_field(subject);
        let subject_cid = escape_copy_field(subject_cid);
        let created_at = escape_copy_field(created_at);
        let indexed_at = escape_copy_field(indexed_at);
        writeln!(
            buffer,
            "{uri}\t{cid}\t{subject}\t{subject_cid}\t{created_at}\t{indexed_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // sortAt is GENERATED ALWAYS; creator is unread by the appview so neither is written.
    let insert_start = Instant::now();
    client
        .execute(
            "INSERT INTO quote (uri, cid, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, subject, subject_cid, created_at, indexed_at
             FROM _bulk_quote
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    let total_ms = setup_ms + copy_ms + insert_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW quote bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms) for {} rows",
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
        let uri = escape_copy_field(uri);
        let cid = escape_copy_field(cid);
        let creator = escape_copy_field(creator);
        let subject = escape_copy_field(subject);
        let created_at = escape_copy_field(created_at);
        let indexed_at = escape_copy_field(indexed_at);
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
        let post_uri = escape_copy_field(post_uri);
        let position = escape_copy_field(position);
        let image_cid = escape_copy_field(image_cid);
        let alt = escape_copy_field(alt);
        writeln!(buffer, "{post_uri}\t{position}\t{image_cid}\t{alt}")
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
        let post_uri = escape_copy_field(post_uri);
        let video_cid = escape_copy_field(video_cid);
        let alt = match alt.as_ref() {
            Some(s) => escape_copy_field(s),
            None => std::borrow::Cow::Borrowed("\\N"),
        };
        writeln!(buffer, "{post_uri}\t{video_cid}\t{alt}")
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
    use super::escape_copy_field;

    #[test]
    fn escapes_backslash_and_whitespace_for_copy() {
        // Backslash is doubled first (it is the COPY escape char), then tab/newline/cr.
        assert_eq!(escape_copy_field("a\\b"), "a\\\\b");
        assert_eq!(
            escape_copy_field("hello\tworld\nline\r"),
            "hello\\tworld\\nline\\r"
        );
        assert_eq!(escape_copy_field("plain text"), "plain text");
    }

    #[test]
    fn escapes_trailing_backslash_so_row_is_not_corrupted() {
        // A trailing backslash previously escaped the tab delimiter and shifted columns.
        assert_eq!(escape_copy_field("path\\"), "path\\\\");
    }
}
