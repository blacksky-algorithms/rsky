use crate::graph::FollowGraph;
use futures::StreamExt;
use rsky_firehose::car;
use rsky_firehose::firehose;
use rsky_lexicon::app::bsky::graph::follow::Follow;
use rsky_lexicon::com::atproto::sync::SubscribeRepos;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const FOLLOW_COLLECTION: &str = "app.bsky.graph.follow";

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
                        process_message(&data, graph);
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

fn process_message(data: &[u8], graph: &FollowGraph) {
    crate::metrics::GRAPH_FIREHOSE_FRAMES.inc();
    // Use the shared rsky-firehose frame parser -- same path as rsky-firehose
    // and rsky-wintermute. Anything other than a #commit is irrelevant here.
    let commit = match firehose::read(data) {
        Ok(Some((_, SubscribeRepos::Commit(c)))) => c,
        Ok(Some(_)) => {
            // Non-commit frame (#identity, #account, #handle, #tombstone, ...)
            return;
        }
        Ok(None) => {
            // #info, #sync, or #error -- silently skip
            return;
        }
        Err(e) => {
            crate::metrics::GRAPH_FIREHOSE_DECODE_ERRORS.inc();
            tracing::debug!("firehose: frame decode failed: {e}");
            return;
        }
    };
    crate::metrics::GRAPH_FIREHOSE_COMMITS.inc();

    if commit.too_big || commit.ops.is_empty() {
        return;
    }

    // Parse the embedded CAR once per commit so multiple follow ops in the
    // same commit share the work.
    let mut block_map: Option<std::collections::HashMap<lexicon_cid::Cid, Vec<u8>>> = None;
    let load_blocks = || -> Option<std::collections::HashMap<lexicon_cid::Cid, Vec<u8>>> {
        let mut reader = Cursor::new(&commit.blocks);
        car::read_header(&mut reader).ok()?;
        car::read_blocks(&mut reader).ok()
    };

    for op in &commit.ops {
        let Some((collection, rkey)) = op.path.split_once('/') else {
            continue;
        };
        if collection != FOLLOW_COLLECTION {
            continue;
        }

        match op.action.as_str() {
            "create" => {
                let Some(cid) = op.cid.as_ref() else { continue };
                if block_map.is_none() {
                    block_map = load_blocks();
                }
                let Some(blocks) = block_map.as_ref() else {
                    continue;
                };
                let Some(record_bytes) = blocks.get(cid) else {
                    continue;
                };
                match serde_cbor::from_reader::<Follow, _>(Cursor::new(record_bytes)) {
                    Ok(follow) => {
                        graph.add_follow_with_rkey(&commit.repo, rkey, &follow.subject);
                        crate::metrics::GRAPH_FIREHOSE_EVENTS.inc();
                    }
                    Err(e) => {
                        tracing::debug!(
                            "firehose: failed to decode follow record at {}/{rkey}: {e}",
                            commit.repo,
                        );
                    }
                }
            }
            "delete" => {
                if graph.remove_follow_by_rkey(&commit.repo, rkey) {
                    crate::metrics::GRAPH_FIREHOSE_EVENTS.inc();
                }
                // If the rkey is unknown (follow predates the rkey index), the
                // bitmap entry stays until the next snapshot rebuild.
            }
            _ => {}
        }
    }
}
