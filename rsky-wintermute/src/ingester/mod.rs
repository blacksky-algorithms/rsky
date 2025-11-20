mod backfill_queue;
mod labels;
mod tests;

use crate::SHUTDOWN;
use crate::config::{FIREHOSE_PING_INTERVAL, WORKERS_INGESTER};
use crate::storage::Storage;
use crate::types::{CommitData, FirehoseEvent, WintermuteError};
use futures::SinkExt;
use futures::stream::StreamExt;
use ipld_core::ipld::Ipld;
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
        let ipld: Ipld = serde_ipld_dagcbor::from_reader(data)
            .map_err(|e| WintermuteError::Serialization(format!("failed to parse ipld: {e}")))?;

        let Ok(Some(header)) = ipld.get("t") else {
            return Ok(None);
        };

        let header_str = match header {
            Ipld::String(s) => s.as_str(),
            _ => return Ok(None),
        };

        if header_str != "#commit" {
            return Ok(None);
        }

        let body = ipld
            .get("op")
            .ok()
            .flatten()
            .ok_or_else(|| WintermuteError::Serialization("missing op field".into()))?;

        let seq = body
            .get("seq")
            .ok()
            .flatten()
            .and_then(|v| match v {
                Ipld::Integer(i) => i64::try_from(*i).ok(),
                _ => None,
            })
            .ok_or_else(|| WintermuteError::Serialization("missing seq".into()))?;

        let did = body
            .get("repo")
            .ok()
            .flatten()
            .and_then(|v| match v {
                Ipld::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| WintermuteError::Serialization("missing repo".into()))?;

        let time = body
            .get("time")
            .ok()
            .flatten()
            .and_then(|v| match v {
                Ipld::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| WintermuteError::Serialization("missing time".into()))?;

        let rev = body
            .get("rev")
            .ok()
            .flatten()
            .and_then(|v| match v {
                Ipld::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| WintermuteError::Serialization("missing rev".into()))?;

        let blocks_data = body
            .get("blocks")
            .ok()
            .flatten()
            .and_then(|v| match v {
                Ipld::Bytes(b) => Some(b.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let event = FirehoseEvent {
            seq,
            did,
            time,
            kind: "commit".to_owned(),
            commit: Some(CommitData {
                rev,
                ops: vec![],
                blocks: blocks_data,
            }),
        };

        Ok(Some(event))
    }
}
