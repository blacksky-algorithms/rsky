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
// NUL is stripped (Postgres text columns reject 0x00), the rest are escaped.
fn escape_copy_field(s: &str) -> std::borrow::Cow<'_, str> {
    if s.bytes()
        .any(|b| matches!(b, b'\\' | b'\t' | b'\n' | b'\r' | b'\0'))
    {
        std::borrow::Cow::Owned(
            s.replace('\\', "\\\\")
                .replace('\t', "\\t")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\0', ""),
        )
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

// Escape an optional COPY field, emitting the \N NULL marker when the value is absent.
fn escape_copy_opt(v: Option<&str>) -> std::borrow::Cow<'_, str> {
    v.map_or(std::borrow::Cow::Borrowed("\\N"), escape_copy_field)
}

/// Render JSON array items as a Postgres array literal (`{"en","es"}`) for
/// `text[]`/`varchar[]` columns. Elements are always double-quoted with `\`
/// and `"` escaped; non-string items are skipped. COPY-level escaping is
/// applied separately by `escape_copy_field`, so the two layers compose.
pub fn pg_text_array_literal(items: &[serde_json::Value]) -> String {
    let mut out = String::from("{");
    let mut first = true;
    for item in items {
        let Some(s) = item.as_str() else { continue };
        if !first {
            out.push(',');
        }
        first = false;
        out.push('"');
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                _ => out.push(c),
            }
        }
        out.push('"');
    }
    out.push('}');
    out
}

/// A post row for bulk `COPY`. `reply_*` are `None` for non-replies; `langs`/`tags`
/// hold Postgres array literals destined for the `varchar[]` columns, `None` when absent.
pub struct PostCopyRow {
    pub uri: String,
    pub cid: String,
    pub creator: String,
    pub text: String,
    pub reply_root: Option<String>,
    pub reply_root_cid: Option<String>,
    pub reply_parent: Option<String>,
    pub reply_parent_cid: Option<String>,
    pub created_at: String,
    pub indexed_at: String,
    pub langs: Option<String>,
    pub tags: Option<String>,
}

/// A like or repost row for bulk `COPY`: strong-ref subject plus optional
/// via attribution.
pub struct SubjectRecordRow {
    pub uri: String,
    pub cid: String,
    pub creator: String,
    pub subject: String,
    pub subject_cid: String,
    pub created_at: String,
    pub indexed_at: String,
    pub via: Option<String>,
    pub via_cid: Option<String>,
}

/// A follow row for bulk `COPY`: DID subject plus optional via attribution.
pub struct FollowCopyRow {
    pub uri: String,
    pub cid: String,
    pub creator: String,
    pub subject_did: String,
    pub created_at: String,
    pub indexed_at: String,
    pub via: Option<String>,
    pub via_cid: Option<String>,
}

/// A notification destined for the `notification` table.
pub struct NotificationRow {
    pub did: String,
    pub author: String,
    pub record_uri: String,
    pub record_cid: String,
    pub reason: &'static str,
    pub reason_subject: Option<String>,
    pub sort_at: String,
}

/// Bulk insert notifications in one statement. Deduped on
/// `(did, "recordUri", reason)` so replays add nothing.
pub async fn copy_insert_notifications(
    client: &deadpool_postgres::Client,
    rows: &[NotificationRow],
) -> Result<(), WintermuteError> {
    if rows.is_empty() {
        return Ok(());
    }
    // Sorted by conflict key so concurrent notification writers acquire
    // unique-index locks in one global order.
    let mut ordered: Vec<&NotificationRow> = rows.iter().collect();
    ordered.sort_unstable_by(|a, b| {
        (&a.did, &a.record_uri, a.reason).cmp(&(&b.did, &b.record_uri, b.reason))
    });
    let mut dids = Vec::with_capacity(rows.len());
    let mut authors = Vec::with_capacity(rows.len());
    let mut uris = Vec::with_capacity(rows.len());
    let mut cids = Vec::with_capacity(rows.len());
    let mut reasons = Vec::with_capacity(rows.len());
    let mut subjects: Vec<Option<&str>> = Vec::with_capacity(rows.len());
    let mut sort_ats = Vec::with_capacity(rows.len());
    for row in ordered {
        dids.push(row.did.as_str());
        authors.push(row.author.as_str());
        uris.push(row.record_uri.as_str());
        cids.push(row.record_cid.as_str());
        reasons.push(row.reason);
        subjects.push(row.reason_subject.as_deref());
        sort_ats.push(row.sort_at.as_str());
    }
    client
        .execute(
            "INSERT INTO notification (did, author, \"recordUri\", \"recordCid\", reason, \"reasonSubject\", \"sortAt\")
             SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[], $7::text[])
             ON CONFLICT (did, \"recordUri\", reason) DO NOTHING",
            &[&dids, &authors, &uris, &cids, &reasons, &subjects, &sort_ats],
        )
        .await?;
    Ok(())
}

/// Sort delta keys so every concurrent aggregate upsert visits rows in one
/// global order; unordered multi-row upserts on shared rows deadlock.
pub fn sorted_deltas(counts: std::collections::HashMap<String, i64>) -> (Vec<String>, Vec<i64>) {
    let mut pairs: Vec<(String, i64)> = counts.into_iter().collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    pairs.into_iter().unzip()
}

/// Increment a `post_agg` count column by exact per-uri deltas.
async fn increment_post_agg(
    client: &deadpool_postgres::Client,
    column: &str,
    counts: std::collections::HashMap<String, i64>,
) -> Result<(), WintermuteError> {
    if counts.is_empty() {
        return Ok(());
    }
    let (uris, deltas) = sorted_deltas(counts);
    client
        .execute(
            &format!(
                "INSERT INTO post_agg (uri, \"{column}\")
                 SELECT * FROM unnest($1::text[], $2::int8[])
                 ON CONFLICT (uri) DO UPDATE
                   SET \"{column}\" = COALESCE(post_agg.\"{column}\", 0) + EXCLUDED.\"{column}\""
            ),
            &[&uris, &deltas],
        )
        .await?;
    Ok(())
}

/// Decrement a `post_agg` count column by exact per-uri deltas, floored at 0.
/// Rows are locked in sorted uri order first so concurrent aggregate writers
/// cannot deadlock; absent rows hydrate as 0 and need no decrement.
pub async fn decrement_post_agg(
    client: &deadpool_postgres::Client,
    column: &str,
    counts: std::collections::HashMap<String, i64>,
) -> Result<(), WintermuteError> {
    if counts.is_empty() {
        return Ok(());
    }
    let (uris, deltas) = sorted_deltas(counts);
    client
        .execute(
            &format!(
                "WITH locked AS (
                     SELECT uri FROM post_agg WHERE uri = ANY($1) ORDER BY uri FOR UPDATE
                 )
                 UPDATE post_agg p
                 SET \"{column}\" = GREATEST(COALESCE(p.\"{column}\", 0) - d.c, 0)
                 FROM unnest($1::text[], $2::int8[]) AS d(u, c)
                 WHERE p.uri = d.u AND p.uri IN (SELECT uri FROM locked)"
            ),
            &[&uris, &deltas],
        )
        .await?;
    Ok(())
}

/// Decrement a `profile_agg` count column by exact per-did deltas, floored at
/// 0, with the same sorted lock acquisition as [`decrement_post_agg`].
pub async fn decrement_profile_agg(
    client: &deadpool_postgres::Client,
    column: &str,
    counts: std::collections::HashMap<String, i64>,
) -> Result<(), WintermuteError> {
    if counts.is_empty() {
        return Ok(());
    }
    let (dids, deltas) = sorted_deltas(counts);
    client
        .execute(
            &format!(
                "WITH locked AS (
                     SELECT did FROM profile_agg WHERE did = ANY($1) ORDER BY did FOR UPDATE
                 )
                 UPDATE profile_agg p
                 SET \"{column}\" = GREATEST(COALESCE(p.\"{column}\", 0) - d.c, 0)
                 FROM unnest($1::text[], $2::int8[]) AS d(u, c)
                 WHERE p.did = d.u AND p.did IN (SELECT did FROM locked)"
            ),
            &[&dids, &deltas],
        )
        .await?;
    Ok(())
}

/// Set-based reply notifications for a batch of new reply posts, replacing
/// per-reply round-trips with one recursive statement over pkey joins. Seeds
/// are `(did, uri, cid, sort_at)` of the new replies; ancestors up to depth 4
/// are notified of each. The per-record path's descendant repair is deliberately
/// omitted here: descendants of a just-created post only exist when indexing
/// ran out of order, the backfill path still repairs that case per-record, and
/// the planner cannot be trusted to bound a descendants recursion over the
/// post table from unnest seeds. Inserts are ordered by conflict key for
/// deadlock-free concurrency and deduped on (did, "recordUri", reason).
pub async fn write_reply_notifications_bulk(
    client: &deadpool_postgres::Client,
    seeds: &[(String, String, String, String)],
) -> Result<(), WintermuteError> {
    if seeds.is_empty() {
        return Ok(());
    }
    let mut dids = Vec::with_capacity(seeds.len());
    let mut uris = Vec::with_capacity(seeds.len());
    let mut cids = Vec::with_capacity(seeds.len());
    let mut sort_ats = Vec::with_capacity(seeds.len());
    for (did, uri, cid, sort_at) in seeds {
        dids.push(did.as_str());
        uris.push(uri.as_str());
        cids.push(cid.as_str());
        sort_ats.push(sort_at.as_str());
    }

    client
        .execute(
            "WITH RECURSIVE seeds AS (
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[])
                     AS s(did, uri, cid, sort_at)
             ),
             ancestor(seed_uri, uri, parent, height) AS (
                 SELECT s.uri, p.uri, p.\"replyParent\", 0
                 FROM seeds s JOIN post p ON p.uri = s.uri
               UNION ALL
                 SELECT a.seed_uri, p.uri, p.\"replyParent\", a.height + 1
                 FROM ancestor a JOIN post p ON p.uri = a.parent
                 WHERE a.height < 4
             )
             INSERT INTO notification (did, author, \"recordUri\", \"recordCid\", reason, \"reasonSubject\", \"sortAt\")
             SELECT split_part(a.uri, '/', 3), s.did, s.uri, s.cid, 'reply', a.uri, s.sort_at
             FROM ancestor a
             JOIN seeds s ON s.uri = a.seed_uri
             WHERE a.height >= 1 AND split_part(a.uri, '/', 3) <> s.did
             ORDER BY 1, 3
             ON CONFLICT (did, \"recordUri\", reason) DO NOTHING",
            &[&dids, &uris, &cids, &sort_ats],
        )
        .await?;

    Ok(())
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
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_record (
                uri text NOT NULL,
                cid text NOT NULL,
                did text NOT NULL,
                json text,
                rev text NOT NULL,
                indexed_at text NOT NULL
            );
            TRUNCATE _bulk_record",
        )
        .await?;
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
             SELECT DISTINCT ON (uri) uri, cid, did, json, rev, indexed_at
             FROM _bulk_record
             ORDER BY uri, rev DESC
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
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_actor (
                did text NOT NULL
            );
            TRUNCATE _bulk_actor",
        )
        .await?;
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
    data: &[PostCopyRow],
    compute_agg: bool, // false for the bulk CAR load (aggregates recomputed in one pass after)
) -> Result<std::collections::HashSet<String>, WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_post (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                text text,
                reply_root text,
                reply_root_cid text,
                reply_parent text,
                reply_parent_cid text,
                created_at text NOT NULL,
                indexed_at text NOT NULL,
                langs text[],
                tags text[]
            );
            TRUNCATE _bulk_post",
        )
        .await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data. text is NOT NULL so empty string is preserved; reply_*/langs/tags
    // use the \N NULL marker when absent.
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_post (uri, cid, creator, text, reply_root, reply_root_cid, reply_parent, reply_parent_cid, created_at, indexed_at, langs, tags) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 300);
    for row in data {
        let uri = escape_copy_field(&row.uri);
        let cid = escape_copy_field(&row.cid);
        let creator = escape_copy_field(&row.creator);
        let text = escape_copy_field(&row.text);
        let reply_root = escape_copy_opt(row.reply_root.as_deref());
        let reply_root_cid = escape_copy_opt(row.reply_root_cid.as_deref());
        let reply_parent = escape_copy_opt(row.reply_parent.as_deref());
        let reply_parent_cid = escape_copy_opt(row.reply_parent_cid.as_deref());
        let created_at = escape_copy_field(&row.created_at);
        let indexed_at = escape_copy_field(&row.indexed_at);
        let langs = escape_copy_opt(row.langs.as_deref());
        let tags = escape_copy_opt(row.tags.as_deref());
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{text}\t{reply_root}\t{reply_root_cid}\t{reply_parent}\t{reply_parent_cid}\t{created_at}\t{indexed_at}\t{langs}\t{tags}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT, returning rows actually inserted so
    // aggregates can increment exactly (dupes/replays add zero) and callers
    // can run per-post side effects only for applied rows.
    let insert_start = Instant::now();
    let inserted = client
        .query(
            "INSERT INTO post (uri, cid, creator, text, \"replyRoot\", \"replyRootCid\", \"replyParent\", \"replyParentCid\", \"createdAt\", \"indexedAt\", langs, tags)
             SELECT uri, cid, creator, text, reply_root, reply_root_cid, reply_parent, reply_parent_cid, created_at, indexed_at, langs::varchar[], tags::varchar[]
             FROM _bulk_post
             ON CONFLICT DO NOTHING
             RETURNING uri, creator, \"replyParent\"",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Phase 4: increment profile_agg postsCount and post_agg replyCount by
    // exact per-key deltas. A full recount here scales with each creator's
    // lifetime post count and dominated batch time on large tables.
    let agg_start = Instant::now();
    let mut applied = std::collections::HashSet::with_capacity(inserted.len());
    if !inserted.is_empty() {
        let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        let mut replies: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for row in &inserted {
            applied.insert(row.get::<_, String>(0));
            *counts.entry(row.get::<_, String>(1)).or_insert(0) += 1;
            if let Some(parent) = row.get::<_, Option<String>>(2) {
                *replies.entry(parent).or_insert(0) += 1;
            }
        }
        if compute_agg {
            let (dids, deltas) = sorted_deltas(counts);
            client
                .execute(
                    "INSERT INTO profile_agg (did, \"postsCount\")
                     SELECT * FROM unnest($1::text[], $2::int8[])
                     ON CONFLICT (did) DO UPDATE
                       SET \"postsCount\" = COALESCE(profile_agg.\"postsCount\", 0) + EXCLUDED.\"postsCount\"",
                    &[&dids, &deltas],
                )
                .await?;
            increment_post_agg(client, "replyCount", replies).await?;
        }
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

    Ok(applied)
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
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_feed_item (
                type text NOT NULL,
                uri text NOT NULL,
                cid text NOT NULL,
                post_uri text NOT NULL,
                originator_did text NOT NULL,
                sort_at text NOT NULL
            );
            TRUNCATE _bulk_feed_item",
        )
        .await?;
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
    data: &[SubjectRecordRow],
    compute_agg: bool, // false for the bulk CAR load (aggregates recomputed in one pass after)
) -> Result<std::collections::HashSet<String>, WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_like (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                subject text NOT NULL,
                subject_cid text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL,
                via text,
                via_cid text
            );
            TRUNCATE _bulk_like",
        )
        .await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_like (uri, cid, creator, subject, subject_cid, created_at, indexed_at, via, via_cid) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 250);
    for row in data {
        let uri = escape_copy_field(&row.uri);
        let cid = escape_copy_field(&row.cid);
        let creator = escape_copy_field(&row.creator);
        let subject = escape_copy_field(&row.subject);
        let subject_cid = escape_copy_field(&row.subject_cid);
        let created_at = escape_copy_field(&row.created_at);
        let indexed_at = escape_copy_field(&row.indexed_at);
        let via = escape_copy_field(row.via.as_deref().unwrap_or(""));
        let via_cid = escape_copy_field(row.via_cid.as_deref().unwrap_or(""));
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{subject}\t{subject_cid}\t{created_at}\t{indexed_at}\t{via}\t{via_cid}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT, returning rows actually inserted so
    // likeCount increments exactly (dupes/replays add zero).
    let insert_start = Instant::now();
    let inserted = client
        .query(
            "INSERT INTO \"like\" (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\", via, \"viaCid\")
             SELECT uri, cid, creator, subject, subject_cid, created_at, indexed_at, via, via_cid
             FROM _bulk_like
             ON CONFLICT DO NOTHING
             RETURNING uri, subject",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    let agg_start = Instant::now();
    let mut applied = std::collections::HashSet::with_capacity(inserted.len());
    let mut likes: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for row in &inserted {
        applied.insert(row.get::<_, String>(0));
        *likes.entry(row.get::<_, String>(1)).or_insert(0) += 1;
    }
    if compute_agg {
        increment_post_agg(client, "likeCount", likes).await?;
    }
    let agg_ms = agg_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms + agg_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW like bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms, agg={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            agg_ms,
            count
        );
    }

    Ok(applied)
}

/// Bulk insert follows using `COPY` protocol.
pub async fn copy_insert_follows(
    client: &deadpool_postgres::Client,
    data: &[FollowCopyRow],
    compute_agg: bool, // false for the bulk CAR load (aggregates recomputed in one pass after)
) -> Result<std::collections::HashSet<String>, WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_follow (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                subject_did text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL,
                via text,
                via_cid text
            );
            TRUNCATE _bulk_follow",
        )
        .await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_follow (uri, cid, creator, subject_did, created_at, indexed_at, via, via_cid) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 200);
    for row in data {
        let uri = escape_copy_field(&row.uri);
        let cid = escape_copy_field(&row.cid);
        let creator = escape_copy_field(&row.creator);
        let subject_did = escape_copy_field(&row.subject_did);
        let created_at = escape_copy_field(&row.created_at);
        let indexed_at = escape_copy_field(&row.indexed_at);
        let via = escape_copy_opt(row.via.as_deref());
        let via_cid = escape_copy_opt(row.via_cid.as_deref());
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{subject_did}\t{created_at}\t{indexed_at}\t{via}\t{via_cid}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT, returning both pair sides of rows
    // actually inserted so aggregates can increment exactly.
    let insert_start = Instant::now();
    let inserted = client
        .query(
            "INSERT INTO follow (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\", via, \"viaCid\")
             SELECT uri, cid, creator, subject_did, created_at, indexed_at, via, via_cid
             FROM _bulk_follow
             ON CONFLICT DO NOTHING
             RETURNING uri, creator, \"subjectDid\"",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    // Phase 4: increment followsCount/followersCount by exact deltas. Full
    // recounts here scale with each account's lifetime follow graph (a
    // popular subject re-counted every follower on every new follow).
    let agg_start = Instant::now();
    let mut applied = std::collections::HashSet::with_capacity(inserted.len());
    for row in &inserted {
        applied.insert(row.get::<_, String>(0));
    }
    if compute_agg && !inserted.is_empty() {
        let mut follows: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        let mut followers: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for row in &inserted {
            *follows.entry(row.get::<_, String>(1)).or_insert(0) += 1;
            *followers.entry(row.get::<_, String>(2)).or_insert(0) += 1;
        }
        let (f_dids, f_deltas) = sorted_deltas(follows);
        client
            .execute(
                "INSERT INTO profile_agg (did, \"followsCount\")
                 SELECT * FROM unnest($1::text[], $2::int8[])
                 ON CONFLICT (did) DO UPDATE
                   SET \"followsCount\" = COALESCE(profile_agg.\"followsCount\", 0) + EXCLUDED.\"followsCount\"",
                &[&f_dids, &f_deltas],
            )
            .await?;
        let (s_dids, s_deltas) = sorted_deltas(followers);
        client
            .execute(
                "INSERT INTO profile_agg (did, \"followersCount\")
                 SELECT * FROM unnest($1::text[], $2::int8[])
                 ON CONFLICT (did) DO UPDATE
                   SET \"followersCount\" = COALESCE(profile_agg.\"followersCount\", 0) + EXCLUDED.\"followersCount\"",
                &[&s_dids, &s_deltas],
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

    Ok(applied)
}

/// Bulk insert reposts using `COPY` protocol.
pub async fn copy_insert_reposts(
    client: &deadpool_postgres::Client,
    data: &[SubjectRecordRow],
    compute_agg: bool, // false for the bulk CAR load (aggregates recomputed in one pass after)
) -> Result<std::collections::HashSet<String>, WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    let count = data.len();

    // Phase 1: Table setup
    let setup_start = Instant::now();
    client
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_repost (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                subject text NOT NULL,
                subject_cid text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL,
                via text,
                via_cid text
            );
            TRUNCATE _bulk_repost",
        )
        .await?;
    let setup_ms = setup_start.elapsed().as_millis();

    // Phase 2: COPY data
    let copy_start = Instant::now();
    let copy_stmt = client
        .copy_in("COPY _bulk_repost (uri, cid, creator, subject, subject_cid, created_at, indexed_at, via, via_cid) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 250);
    for row in data {
        let uri = escape_copy_field(&row.uri);
        let cid = escape_copy_field(&row.cid);
        let creator = escape_copy_field(&row.creator);
        let subject = escape_copy_field(&row.subject);
        let subject_cid = escape_copy_field(&row.subject_cid);
        let created_at = escape_copy_field(&row.created_at);
        let indexed_at = escape_copy_field(&row.indexed_at);
        let via = escape_copy_field(row.via.as_deref().unwrap_or(""));
        let via_cid = escape_copy_field(row.via_cid.as_deref().unwrap_or(""));
        writeln!(
            buffer,
            "{uri}\t{cid}\t{creator}\t{subject}\t{subject_cid}\t{created_at}\t{indexed_at}\t{via}\t{via_cid}"
        )
        .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    let copy_ms = copy_start.elapsed().as_millis();

    // Phase 3: INSERT...ON CONFLICT, returning rows actually inserted so
    // repostCount increments exactly (dupes/replays add zero).
    let insert_start = Instant::now();
    let inserted = client
        .query(
            "INSERT INTO repost (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\", via, \"viaCid\")
             SELECT uri, cid, creator, subject, subject_cid, created_at, indexed_at, via, via_cid
             FROM _bulk_repost
             ON CONFLICT DO NOTHING
             RETURNING uri, subject",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    let agg_start = Instant::now();
    let mut applied = std::collections::HashSet::with_capacity(inserted.len());
    let mut reposts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for row in &inserted {
        applied.insert(row.get::<_, String>(0));
        *reposts.entry(row.get::<_, String>(1)).or_insert(0) += 1;
    }
    if compute_agg {
        increment_post_agg(client, "repostCount", reposts).await?;
    }
    let agg_ms = agg_start.elapsed().as_millis();

    // Log if total > 100ms (worth investigating)
    let total_ms = setup_ms + copy_ms + insert_ms + agg_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW repost bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms, agg={}ms) for {} rows",
            total_ms,
            setup_ms,
            copy_ms,
            insert_ms,
            agg_ms,
            count
        );
    }

    Ok(applied)
}

/// Bulk insert quotes using `COPY` protocol.
pub async fn copy_insert_quotes(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, subject, subject_cid, created_at, indexed_at
    compute_agg: bool, // false for the bulk CAR load (aggregates recomputed in one pass after)
) -> Result<(), WintermuteError> {
    use std::time::Instant;

    if data.is_empty() {
        return Ok(());
    }

    let count = data.len();

    let setup_start = Instant::now();
    client
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_quote (
                uri text NOT NULL,
                cid text NOT NULL,
                subject text NOT NULL,
                subject_cid text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL
            );
            TRUNCATE _bulk_quote",
        )
        .await?;
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
    let inserted = client
        .query(
            "INSERT INTO quote (uri, cid, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, subject, subject_cid, created_at, indexed_at
             FROM _bulk_quote
             ON CONFLICT DO NOTHING
             RETURNING subject",
            &[],
        )
        .await?;
    let insert_ms = insert_start.elapsed().as_millis();

    let agg_start = Instant::now();
    if compute_agg {
        let mut quotes: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for row in &inserted {
            *quotes.entry(row.get::<_, String>(0)).or_insert(0) += 1;
        }
        increment_post_agg(client, "quoteCount", quotes).await?;
    }
    let agg_ms = agg_start.elapsed().as_millis();

    let total_ms = setup_ms + copy_ms + insert_ms + agg_ms;
    if total_ms > 100 {
        tracing::warn!(
            "SLOW quote bulk: {}ms total (setup={}ms, copy={}ms, insert={}ms, agg={}ms) for {} rows",
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
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_block (
                uri text NOT NULL,
                cid text NOT NULL,
                creator text NOT NULL,
                subject text NOT NULL,
                created_at text NOT NULL,
                indexed_at text NOT NULL
            );
            TRUNCATE _bulk_block",
        )
        .await?;
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
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_post_embed_image (
                post_uri text NOT NULL,
                position text NOT NULL,
                image_cid text NOT NULL,
                alt text NOT NULL
            );
            TRUNCATE _bulk_post_embed_image",
        )
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
        .batch_execute(
            "CREATE TEMP TABLE IF NOT EXISTS _bulk_post_embed_video (
                post_uri text NOT NULL,
                video_cid text NOT NULL,
                alt text
            );
            TRUNCATE _bulk_post_embed_video",
        )
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
        let alt = alt
            .as_ref()
            .map_or(std::borrow::Cow::Borrowed("\\N"), |s| escape_copy_field(s));
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
    use super::{escape_copy_field, escape_copy_opt, pg_text_array_literal};

    #[test]
    fn array_literal_renders_quoted_elements() {
        let items = vec![serde_json::json!("en"), serde_json::json!("pt-BR")];
        assert_eq!(pg_text_array_literal(&items), r#"{"en","pt-BR"}"#);
    }

    #[test]
    fn array_literal_escapes_quotes_and_backslashes() {
        let items = vec![
            serde_json::json!(r#"tag "quoted""#),
            serde_json::json!(r"back\slash"),
            serde_json::json!("with,comma"),
            serde_json::json!("with{brace}"),
        ];
        assert_eq!(
            pg_text_array_literal(&items),
            r#"{"tag \"quoted\"","back\\slash","with,comma","with{brace}"}"#
        );
    }

    #[test]
    fn array_literal_empty_and_non_string_items() {
        assert_eq!(pg_text_array_literal(&[]), "{}");
        let items = vec![
            serde_json::json!(42),
            serde_json::json!("kept"),
            serde_json::json!(null),
        ];
        assert_eq!(pg_text_array_literal(&items), r#"{"kept"}"#);
    }

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

    #[test]
    fn optional_field_emits_null_marker_when_absent() {
        // None -> \N (COPY NULL); Some -> escaped value, so reply_*/langs/tags load correctly.
        assert_eq!(escape_copy_opt(None), "\\N");
        assert_eq!(
            escape_copy_opt(Some("at://did/app.bsky.feed.post/x")),
            "at://did/app.bsky.feed.post/x"
        );
        assert_eq!(escape_copy_opt(Some("a\tb")), "a\\tb");
    }
}
