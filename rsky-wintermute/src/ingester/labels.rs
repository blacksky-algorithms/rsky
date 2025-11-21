use crate::types::{Label, LabelEvent, WintermuteError};
use crate::{SHUTDOWN, metrics, storage::Storage};
use futures::SinkExt;
use futures::stream::StreamExt;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::interval;
use tokio_tungstenite::tungstenite::Message;

pub async fn subscribe_labels(
    storage: Arc<Storage>,
    labeler_host: String,
) -> Result<(), WintermuteError> {
    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            tracing::info!("shutdown requested for label subscriber {labeler_host}");
            break;
        }

        match connect_and_stream(&storage, &labeler_host).await {
            Ok(()) => {
                tracing::warn!("label connection closed for {labeler_host}, reconnecting in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(e) => {
                tracing::error!("label connection error for {labeler_host}: {e}, retrying in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
    Ok(())
}

async fn connect_and_stream(storage: &Storage, labeler_host: &str) -> Result<(), WintermuteError> {
    let cursor_key = format!("labels:{labeler_host}");
    let cursor = storage.get_cursor(&cursor_key)?.unwrap_or(0);

    let clean_hostname = labeler_host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');

    let mut url = url::Url::parse(&format!(
        "wss://{clean_hostname}/xrpc/com.atproto.label.subscribeLabels"
    ))
    .map_err(|e| WintermuteError::Other(format!("invalid url: {e}")))?;

    url.query_pairs_mut()
        .append_pair("cursor", &cursor.to_string());

    tracing::info!("connecting to label stream at {url} with cursor {cursor}");

    let (ws_stream, _) = tokio_tungstenite::connect_async(url.as_str()).await?;
    let (mut write, mut read) = ws_stream.split();

    // Track active connection
    metrics::INGESTER_WEBSOCKET_CONNECTIONS
        .with_label_values(&["labels"])
        .inc();

    let ping_task = tokio::spawn(async move {
        let mut ping_interval = interval(Duration::from_secs(30));
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
            match parse_label_message(&data) {
                Ok(Some(label_event)) => {
                    // Update cursor
                    if let Err(e) = storage.set_cursor(&cursor_key, label_event.seq) {
                        tracing::error!("failed to set label cursor: {e}");
                        metrics::INGESTER_ERRORS_TOTAL
                            .with_label_values(&["label_cursor"])
                            .inc();
                        continue;
                    }

                    // Enqueue label event to label_live queue
                    if let Err(e) = storage.enqueue_label_live(&label_event) {
                        tracing::error!("failed to enqueue label event: {e}");
                        metrics::INGESTER_ERRORS_TOTAL
                            .with_label_values(&["label_enqueue"])
                            .inc();
                        continue;
                    }

                    // Update metrics
                    metrics::INGESTER_FIREHOSE_EVENTS_TOTAL
                        .with_label_values(&["label_live"])
                        .inc();

                    tracing::debug!(
                        "enqueued label event seq={} with {} labels",
                        label_event.seq,
                        label_event.labels.len()
                    );
                }
                Ok(None) => {
                    // Not a label message, skip
                }
                Err(e) => {
                    tracing::error!("failed to parse label message: {e}");
                    metrics::INGESTER_ERRORS_TOTAL
                        .with_label_values(&["label_parse"])
                        .inc();
                }
            }
        }
    }

    ping_task.abort();

    // Track connection closed
    metrics::INGESTER_WEBSOCKET_CONNECTIONS
        .with_label_values(&["labels"])
        .dec();

    Ok(())
}

pub fn parse_label_message(data: &[u8]) -> Result<Option<LabelEvent>, WintermuteError> {
    // AT Protocol sends two concatenated CBOR messages:
    // 1. Header (parsed with ciborium): {t: "#labels", op: 1}
    // 2. Body (parsed with serde_ipld_dagcbor): {seq: N, labels: [...]}

    #[derive(serde::Deserialize)]
    struct Header {
        #[serde(rename = "t")]
        type_: String,
        #[serde(rename = "op")]
        _operation: u8,
    }

    #[derive(serde::Deserialize)]
    struct SubscribeLabels {
        seq: i64,
        labels: Vec<RawLabel>,
    }

    #[derive(serde::Deserialize)]
    struct RawLabel {
        src: String,
        uri: String,
        val: String,
        #[allow(dead_code)]
        #[serde(default)]
        cid: Option<String>,
        cts: String,
    }

    let mut cursor = std::io::Cursor::new(data);

    // Parse header with ciborium
    let header: Header = ciborium::from_reader(&mut cursor)
        .map_err(|e| WintermuteError::Serialization(format!("failed to parse header: {e}")))?;

    if header.type_ != "#labels" {
        return Ok(None);
    }

    // Parse body with serde_ipld_dagcbor
    let body: SubscribeLabels = serde_ipld_dagcbor::from_reader(&mut cursor)
        .map_err(|e| WintermuteError::Serialization(format!("failed to parse body: {e}")))?;

    let labels = body
        .labels
        .into_iter()
        .map(|raw| Label {
            src: raw.src,
            uri: raw.uri,
            val: raw.val,
            cts: raw.cts,
        })
        .collect();

    Ok(Some(LabelEvent {
        seq: body.seq,
        labels,
    }))
}
