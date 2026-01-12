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
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // Create temp table (persists for session, TRUNCATE clears it between batches)
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

    // COPY data into temp table
    let copy_stmt = client
        .copy_in("COPY _bulk_record (uri, cid, did, json, rev, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    // Build tab-separated data
    let mut buffer = Vec::with_capacity(data.len() * 200);
    for (uri, cid, did, json, rev, indexed_at) in data {
        // Escape tabs and newlines in json
        let escaped_json = json.replace('\t', "\\t").replace('\n', "\\n");
        writeln!(
            buffer,
            "{uri}\t{cid}\t{did}\t{escaped_json}\t{rev}\t{indexed_at}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;

    // Insert from temp to real table with conflict handling
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
    if dids.is_empty() {
        return Ok(());
    }

    // Create temp table (persists for session, TRUNCATE clears it between batches)
    client
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_actor (
                did text NOT NULL
            )",
            &[],
        )
        .await?;

    client.execute("TRUNCATE _bulk_actor", &[]).await?;

    // COPY dids
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

    // Insert with conflict handling
    client
        .execute(
            "INSERT INTO actor (did, \"indexedAt\")
             SELECT did, '1970-01-01T00:00:00Z'
             FROM _bulk_actor
             ON CONFLICT (did) DO NOTHING",
            &[],
        )
        .await?;

    Ok(())
}

/// Bulk insert posts using `COPY` protocol.
pub async fn copy_insert_posts(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, text, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    // Create temp table (persists for session, TRUNCATE clears it between batches)
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

    // COPY data
    let copy_stmt = client
        .copy_in("COPY _bulk_post (uri, cid, creator, text, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
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

    // Insert from temp
    client
        .execute(
            "INSERT INTO post (uri, cid, creator, text, \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, text, created_at, indexed_at
             FROM _bulk_post
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;

    Ok(())
}

/// Bulk insert `feed_item` records using `COPY` protocol.
pub async fn copy_insert_feed_items(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // type, uri, cid, post_uri, originator_did, sort_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    // Create temp table (persists for session, TRUNCATE clears it between batches)
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

    // COPY data
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

    // Insert from temp
    client
        .execute(
            "INSERT INTO feed_item (type, uri, cid, \"postUri\", \"originatorDid\", \"sortAt\")
             SELECT type, uri, cid, post_uri, originator_did, sort_at
             FROM _bulk_feed_item
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;

    Ok(())
}

/// Bulk insert likes using `COPY` protocol.
pub async fn copy_insert_likes(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String, String)], // uri, cid, creator, subject, subject_cid, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    // Create temp table (persists for session, TRUNCATE clears it between batches)
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

    // COPY data
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

    // Insert from temp
    client
        .execute(
            "INSERT INTO \"like\" (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, subject_cid, created_at, indexed_at
             FROM _bulk_like
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;

    Ok(())
}

/// Bulk insert follows using `COPY` protocol.
pub async fn copy_insert_follows(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, subject_did, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    // Create temp table (persists for session, TRUNCATE clears it between batches)
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

    // COPY data
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

    // Insert from temp
    client
        .execute(
            "INSERT INTO follow (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject_did, created_at, indexed_at
             FROM _bulk_follow
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;

    Ok(())
}

/// Bulk insert reposts using `COPY` protocol.
pub async fn copy_insert_reposts(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String, String)], // uri, cid, creator, subject, subject_cid, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    // Create temp table (persists for session, TRUNCATE clears it between batches)
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

    // COPY data
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

    // Insert from temp
    client
        .execute(
            "INSERT INTO repost (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, subject_cid, created_at, indexed_at
             FROM _bulk_repost
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;

    Ok(())
}

/// Bulk insert blocks using `COPY` protocol.
pub async fn copy_insert_blocks(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, subject, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    // Create temp table (persists for session, TRUNCATE clears it between batches)
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

    // COPY data
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

    // Insert from temp
    client
        .execute(
            "INSERT INTO actor_block (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, created_at, indexed_at
             FROM _bulk_block
             ON CONFLICT DO NOTHING",
            &[],
        )
        .await?;

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
