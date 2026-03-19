//! Staging COPY functions for offline build + sorted merge backfill.
//!
//! These write directly into UNLOGGED staging tables with no indexes,
//! no constraints, and no ON CONFLICT logic. Pure sequential append.
//! Deduplication happens at merge time via ON CONFLICT DO NOTHING.

use crate::types::WintermuteError;
use futures::SinkExt;
use futures::pin_mut;
use std::io::Write;

pub async fn staging_copy_records(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, did, json, rev, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_record (uri, cid, did, json, rev, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 200);
    for (uri, cid, did, json, rev, indexed_at) in data {
        let json = json.replace('\0', "").replace("\\u0000", "");

        if serde_json::from_str::<serde_json::Value>(&json).is_err() {
            tracing::error!("staging: skipping {uri} - invalid JSON");
            continue;
        }

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
    Ok(())
}

pub async fn staging_copy_actors(
    client: &deadpool_postgres::Client,
    dids: &[&str],
) -> Result<(), WintermuteError> {
    if dids.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_actor (did) FROM STDIN WITH (FORMAT text)")
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
    Ok(())
}

pub async fn staging_copy_posts(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, text, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_post (uri, cid, creator, text, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
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
    Ok(())
}

pub async fn staging_copy_feed_items(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // type, uri, cid, post_uri, originator_did, sort_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_feed_item (type, uri, cid, post_uri, originator_did, sort_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
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
    Ok(())
}

pub async fn staging_copy_likes(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String, String)], // uri, cid, creator, subject, subject_cid, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_like (uri, cid, creator, subject, subject_cid, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
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
    Ok(())
}

pub async fn staging_copy_follows(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, subject_did, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_follow (uri, cid, creator, subject_did, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
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
    Ok(())
}

pub async fn staging_copy_reposts(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String, String)], // uri, cid, creator, subject, subject_cid, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_repost (uri, cid, creator, subject, subject_cid, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '')")
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
    Ok(())
}

pub async fn staging_copy_blocks(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String, String, String)], // uri, cid, creator, subject, created_at, indexed_at
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_block (uri, cid, creator, subject, created_at, indexed_at) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
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
    Ok(())
}

pub async fn staging_copy_embed_images(
    client: &deadpool_postgres::Client,
    data: &[(String, String, String, String)], // post_uri, position, image_cid, alt
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_post_embed_image (post_uri, position, image_cid, alt) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 150);
    for (post_uri, position, image_cid, alt) in data {
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
    Ok(())
}

pub async fn staging_copy_embed_videos(
    client: &deadpool_postgres::Client,
    data: &[(String, String, Option<String>)], // post_uri, video_cid, alt
) -> Result<(), WintermuteError> {
    if data.is_empty() {
        return Ok(());
    }

    let copy_stmt = client
        .copy_in("COPY staging_post_embed_video (post_uri, video_cid, alt) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '\\N')")
        .await?;

    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(data.len() * 150);
    for (post_uri, video_cid, alt) in data {
        let escaped_alt = alt.as_ref().map_or_else(
            || "\\N".to_owned(),
            |a| {
                a.chars()
                    .map(|c| match c {
                        '\t' | '\n' | '\r' => ' ',
                        _ => c,
                    })
                    .collect::<String>()
            },
        );
        writeln!(buffer, "{post_uri}\t{video_cid}\t{escaped_alt}")
            .map_err(|e| WintermuteError::Other(format!("buffer write error: {e}")))?;
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_staging_tsv_escape() {
        let text = "hello\tworld\nnewline";
        let escaped = text.replace(['\t', '\n'], " ");
        assert_eq!(escaped, "hello world newline");
    }
}
