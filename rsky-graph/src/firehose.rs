use crate::graph::FollowGraph;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

pub async fn tail_firehose(relay_host: &str, graph: &FollowGraph, shutdown: &AtomicBool) {
    let mut backoff_ms = 1000u64;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            tracing::info!("firehose: shutdown requested");
            return;
        }

        let url = format!("{relay_host}/xrpc/com.atproto.sync.subscribeRepos");
        tracing::info!("firehose: connecting to {url}");

        match tokio_tungstenite::connect_async(&url).await {
            Ok((ws_stream, _)) => {
                backoff_ms = 1000;
                tracing::info!("firehose: connected");
                let (_, mut read) = ws_stream.split();

                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }

                    let msg = tokio::select! {
                        msg = read.next() => {
                            match msg {
                                Some(Ok(m)) => m,
                                Some(Err(e)) => {
                                    tracing::warn!("firehose: read error: {e}");
                                    break;
                                }
                                None => break,
                            }
                        }
                        () = tokio::time::sleep(Duration::from_millis(100)) => continue,
                    };

                    if let Message::Binary(data) = msg {
                        process_firehose_message(&data, graph);
                    }
                }
            }
            Err(e) => {
                tracing::error!("firehose: connect error: {e}");
            }
        }

        if shutdown.load(Ordering::Relaxed) {
            return;
        }

        tracing::warn!("firehose: reconnecting in {backoff_ms}ms");
        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(60_000);
    }
}

fn process_firehose_message(data: &[u8], graph: &FollowGraph) {
    // Parse AT Protocol CBOR frame: header + body
    #[derive(serde::Deserialize)]
    struct Header {
        #[serde(rename = "t")]
        type_: String,
        #[serde(rename = "op")]
        _operation: i8,
    }

    #[derive(serde::Deserialize)]
    struct Commit {
        repo: String,
        ops: Vec<Op>,
        #[serde(with = "serde_bytes")]
        blocks: Vec<u8>,
    }

    #[derive(serde::Deserialize)]
    struct Op {
        action: String,
        path: String,
        cid: Option<serde_json::Value>,
    }

    let mut cursor = std::io::Cursor::new(data);

    let header: Header = match ciborium::from_reader(&mut cursor) {
        Ok(h) => h,
        Err(_) => return,
    };

    if header.type_ != "#commit" {
        return;
    }

    let commit: Commit = match serde_ipld_dagcbor::from_reader(&mut cursor) {
        Ok(c) => c,
        Err(_) => return,
    };

    for op in &commit.ops {
        let parts: Vec<&str> = op.path.split('/').collect();
        if parts.len() != 2 {
            continue;
        }
        let collection = parts[0];
        if collection != "app.bsky.graph.follow" {
            continue;
        }

        match op.action.as_str() {
            "create" => {
                // Parse the record from blocks to get the subject DID
                if let Some(subject) = extract_follow_subject(&commit.blocks, &op.cid) {
                    graph.add_follow(&commit.repo, &subject);

                    // Update bloom filter for the subject
                    if let Some(subject_uid) = graph.get_uid(&subject) {
                        if let Some(actor_uid) = graph.get_uid(&commit.repo) {
                            graph
                                .follower_blooms
                                .entry(subject_uid)
                                .or_insert_with(|| crate::bloom::new_bloom_filter(100))
                                .set(&actor_uid);
                        }
                    }

                    crate::metrics::GRAPH_FIREHOSE_EVENTS.inc();
                }
            }
            "delete" => {
                // For deletes, we need the subject DID. Since the firehose doesn't
                // include the record content for deletes, we check if we can find
                // the subject from our graph data.
                // The path is "app.bsky.graph.follow/{rkey}" -- we don't know the subject.
                // For now, skip deletes. The graph will have stale follows until
                // a periodic full rebuild or a PostgreSQL lookup is added.
                // This is acceptable because:
                // 1. Unfollows are less frequent than follows
                // 2. A stale follow in the graph means we might show a "known follower"
                //    who actually unfollowed -- a minor inaccuracy
                // 3. Periodic LMDB persistence + bloom rebuild corrects over time
            }
            _ => {}
        }
    }
}

fn extract_follow_subject(blocks: &[u8], _cid: &Option<serde_json::Value>) -> Option<String> {
    // The blocks field in a commit contains CAR-encoded data.
    // For follow records, the record has a "subject" field with the DID.
    // We scan the raw bytes for a CBOR-encoded record with a "subject" key
    // that starts with "did:". This is a pragmatic approach that avoids
    // full CAR parsing overhead in the hot path.

    // Try DAG-CBOR parse of the blocks looking for follow records
    // The blocks may contain multiple CBOR items; we scan for any
    // that have a "subject" field starting with "did:"
    let block_str = String::from_utf8_lossy(blocks);

    // Quick scan for "did:plc:" or "did:web:" substrings near a "subject" key
    // This is a fast heuristic -- the proper approach would be CAR parsing
    for window in blocks.windows(200) {
        // Look for CBOR string "subject" followed by a DID
        if let Ok(val) = serde_ipld_dagcbor::from_slice::<serde_json::Value>(window) {
            if let Some(subject) = val.get("subject").and_then(|s| s.as_str()) {
                if subject.starts_with("did:") {
                    return Some(subject.to_owned());
                }
            }
        }
    }

    // Fallback: try parsing the entire blocks as a single CBOR value
    if let Ok(val) = serde_ipld_dagcbor::from_slice::<serde_json::Value>(blocks) {
        if let Some(subject) = val.get("subject").and_then(|s| s.as_str()) {
            if subject.starts_with("did:") {
                return Some(subject.to_owned());
            }
        }
    }

    None
}
