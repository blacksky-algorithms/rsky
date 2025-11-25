pub mod backfill_queue;
#[cfg(test)]
mod backfill_queue_tests;
pub mod labels;
mod tests;

use crate::SHUTDOWN;
use crate::config::{FIREHOSE_PING_INTERVAL, WORKERS_INGESTER};
use crate::storage::Storage;
use crate::types::{CommitData, FirehoseEvent, WintermuteError};
use futures::SinkExt;
use futures::stream::StreamExt;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::interval;
use tokio_tungstenite::tungstenite::Message;

pub struct IngesterManager {
    workers: usize,
    relay_hosts: Vec<String>,
    labeler_hosts: Vec<String>,
    storage: Arc<Storage>,
}

impl IngesterManager {
    pub const fn new(
        relay_hosts: Vec<String>,
        labeler_hosts: Vec<String>,
        storage: Arc<Storage>,
    ) -> Result<Self, WintermuteError> {
        Ok(Self {
            workers: WORKERS_INGESTER,
            relay_hosts,
            labeler_hosts,
            storage,
        })
    }

    pub fn run(self) -> Result<(), WintermuteError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(self.workers)
            .enable_all()
            .build()
            .map_err(|e| WintermuteError::Other(format!("failed to create runtime: {e}")))?;

        rt.block_on(async {
            let mut tasks = Vec::new();

            for host in &self.relay_hosts {
                let storage = Arc::clone(&self.storage);
                let host_clone = host.clone();

                let firehose_task = tokio::spawn(async move {
                    Self::run_connection(Arc::clone(&storage), host_clone.clone()).await;
                });
                tasks.push(firehose_task);

                let backfill_storage = Arc::clone(&self.storage);
                let backfill_host = host.clone();
                let backfill_task = tokio::spawn(async move {
                    if let Err(e) =
                        backfill_queue::populate_backfill_queue(backfill_storage, backfill_host)
                            .await
                    {
                        tracing::error!("backfill queue population failed: {e}");
                    }
                });
                tasks.push(backfill_task);
            }

            for host in &self.labeler_hosts {
                let storage = Arc::clone(&self.storage);
                let host = host.clone();

                let task = tokio::spawn(async move {
                    if let Err(e) = labels::subscribe_labels(storage, host).await {
                        tracing::error!("label subscription failed: {e}");
                    }
                });

                tasks.push(task);
            }

            for task in tasks {
                drop(task.await);
            }
        });

        Ok(())
    }

    async fn run_connection(storage: Arc<Storage>, hostname: String) {
        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!("shutdown requested for {hostname}");
                break;
            }

            match Self::connect_and_stream(&storage, &hostname).await {
                Ok(()) => {
                    tracing::warn!("connection closed for {hostname}, reconnecting in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Err(e) => {
                    tracing::error!("connection error for {hostname}: {e}, retrying in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn connect_and_stream(storage: &Storage, hostname: &str) -> Result<(), WintermuteError> {
        use crate::metrics;

        let cursor_key = format!("firehose:{hostname}");
        let cursor = storage.get_cursor(&cursor_key)?.unwrap_or(0);

        let clean_hostname = hostname
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');

        let mut url = url::Url::parse(&format!(
            "wss://{clean_hostname}/xrpc/com.atproto.sync.subscribeRepos"
        ))
        .map_err(|e| WintermuteError::Other(format!("invalid url: {e}")))?;

        url.query_pairs_mut()
            .append_pair("cursor", &cursor.to_string());

        tracing::info!("connecting to {url} with cursor {cursor}");

        let (ws_stream, _) = tokio_tungstenite::connect_async(url.as_str()).await?;
        metrics::INGESTER_WEBSOCKET_CONNECTIONS
            .with_label_values(&["firehose"])
            .inc();
        let (mut write, mut read) = ws_stream.split();

        let ping_task = tokio::spawn(async move {
            let mut ping_interval = interval(FIREHOSE_PING_INTERVAL);
            loop {
                ping_interval.tick().await;
                if write.send(Message::Ping(vec![])).await.is_err() {
                    break;
                }
            }
        });

        while let Some(msg_result) = read.next().await {
            let msg = msg_result?;

            if let Message::Binary(data) = msg {
                if let Some(event) = Self::parse_message(&data)? {
                    metrics::INGESTER_FIREHOSE_EVENTS_TOTAL
                        .with_label_values(&["firehose_live"])
                        .inc();

                    // Store the raw event for historical lookup
                    storage.write_firehose_event(event.seq, &event)?;

                    // Convert event to index jobs and enqueue for indexer
                    if let Err(e) = Self::enqueue_event_for_indexing(storage, &event).await {
                        tracing::error!(
                            "failed to enqueue event seq={} for indexing: {e}",
                            event.seq
                        );
                        metrics::INGESTER_ERRORS_TOTAL
                            .with_label_values(&["enqueue_failed"])
                            .inc();
                    }

                    storage.set_cursor(&cursor_key, event.seq)?;
                }
            }
        }

        metrics::INGESTER_WEBSOCKET_CONNECTIONS
            .with_label_values(&["firehose"])
            .dec();
        ping_task.abort();
        Ok(())
    }

    fn parse_message(data: &[u8]) -> Result<Option<FirehoseEvent>, WintermuteError> {
        // AT Protocol sends two concatenated CBOR messages:
        // 1. Header (parsed with ciborium): {t: "#commit", op: 1}
        // 2. Body (parsed with serde_ipld_dagcbor): {seq: N, repo: "...", ...}

        #[derive(serde::Deserialize)]
        struct Header {
            #[serde(rename = "t")]
            type_: String,
            #[serde(rename = "op")]
            _operation: u8,
        }

        let mut cursor = std::io::Cursor::new(data);

        // Parse header with ciborium
        let header: Header = ciborium::from_reader(&mut cursor)
            .map_err(|e| WintermuteError::Serialization(format!("failed to parse header: {e}")))?;

        // Only process #commit messages for now
        if header.type_ != "#commit" {
            return Ok(None);
        }

        // Parse body with serde_ipld_dagcbor using the official SubscribeReposCommit struct
        let body: rsky_lexicon::com::atproto::sync::SubscribeReposCommit =
            serde_ipld_dagcbor::from_reader(&mut cursor).map_err(|e| {
                WintermuteError::Serialization(format!("failed to parse body: {e}"))
            })?;

        // Convert operations to our format
        let ops = body
            .ops
            .into_iter()
            .map(|op| crate::types::RepoOp {
                action: op.action,
                path: op.path,
                cid: op.cid.map(|c| c.to_string()),
            })
            .collect();

        let event = FirehoseEvent {
            seq: body.seq,
            did: body.repo,
            time: body.time.to_rfc3339(),
            kind: "commit".to_owned(),
            commit: Some(CommitData {
                rev: body.rev,
                ops,
                blocks: body.blocks,
            }),
        };

        Ok(Some(event))
    }

    pub async fn enqueue_event_for_indexing(
        storage: &Storage,
        event: &FirehoseEvent,
    ) -> Result<(), WintermuteError> {
        use rsky_repo::parse::get_and_parse_record;

        // Only process commit events with operations
        let Some(ref commit) = event.commit else {
            return Ok(());
        };

        if commit.ops.is_empty() {
            return Ok(());
        }

        // Parse CAR blocks into a BlockMap
        let block_map = if commit.blocks.is_empty() {
            None
        } else {
            let cursor = std::io::Cursor::new(&commit.blocks);
            match iroh_car::CarReader::new(cursor).await {
                Ok(mut car_reader) => {
                    let mut block_map = rsky_repo::block_map::BlockMap::new();
                    while let Ok(Some((cid, data))) = car_reader.next_block().await {
                        block_map.set(cid, data);
                    }
                    Some(block_map)
                }
                Err(e) => {
                    tracing::warn!("failed to parse CAR blocks for seq={}: {e}", event.seq);
                    None
                }
            }
        };

        let indexed_at = chrono::Utc::now().to_rfc3339();

        // Convert each operation to an IndexJob
        for op in &commit.ops {
            let action = match op.action.as_str() {
                "create" => crate::types::WriteAction::Create,
                "update" => crate::types::WriteAction::Update,
                "delete" => crate::types::WriteAction::Delete,
                _ => {
                    tracing::warn!("unknown action type: {}", op.action);
                    continue;
                }
            };

            // Build AT-URI from repo DID + record path
            let uri = format!("at://{}/{}", event.did, op.path);

            // Use CID from operation, or empty string for deletes
            let cid_str = op.cid.clone().unwrap_or_default();

            // Extract record from CAR blocks if we have a CID and block_map
            let record = if let (Some(cid_value), Some(blocks)) = (&op.cid, &block_map) {
                match lexicon_cid::Cid::try_from(cid_value.as_str()) {
                    Ok(cid) => match get_and_parse_record(blocks, cid) {
                        Ok(parsed) => match serde_json::to_value(&parsed.record) {
                            Ok(record_json) => Some(record_json),
                            Err(e) => {
                                tracing::warn!("failed to serialize record for {uri}: {e}");
                                None
                            }
                        },
                        Err(e) => {
                            tracing::warn!("failed to parse record for {uri}: {e}");
                            None
                        }
                    },
                    Err(e) => {
                        tracing::warn!("invalid CID {cid_value} for {uri}: {e}");
                        None
                    }
                }
            } else {
                None
            };

            let job = crate::types::IndexJob {
                uri,
                cid: cid_str,
                action,
                record,
                indexed_at: indexed_at.clone(),
                rev: commit.rev.clone(),
            };

            storage.enqueue_firehose_live(&job)?;
        }

        Ok(())
    }
}
