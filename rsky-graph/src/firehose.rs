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
        let (collection, rkey) = (parts[0], parts[1]);
        if collection != "app.bsky.graph.follow" {
            continue;
        }

        match op.action.as_str() {
            "create" => {
                if let Some(subject) = extract_follow_subject(&commit.blocks, &op.cid) {
                    graph.add_follow_with_rkey(&commit.repo, rkey, &subject);
                    crate::metrics::GRAPH_FIREHOSE_EVENTS.inc();
                }
            }
            "delete" => {
                if graph.remove_follow_by_rkey(&commit.repo, rkey) {
                    crate::metrics::GRAPH_FIREHOSE_EVENTS.inc();
                }
                // If we don't know the rkey (pre-snapshot follow), the bitmap
                // entry stays until the next snapshot rebuild. Acceptable.
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

    // Try DAG-CBOR parse of the blocks looking for follow records.
    // The blocks may contain multiple CBOR items; we scan for any
    // that have a "subject" field starting with "did:". This is a fast heuristic --
    // the proper approach would be full CAR parsing.
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
