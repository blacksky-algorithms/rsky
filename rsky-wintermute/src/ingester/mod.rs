mod backfill_queue;
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
                    storage.write_firehose_event(event.seq, &event)?;
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
}
