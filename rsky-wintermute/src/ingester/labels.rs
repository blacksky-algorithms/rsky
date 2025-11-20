use crate::types::{Label, LabelEvent, WintermuteError};
use crate::{SHUTDOWN, metrics, storage::Storage};
use futures::SinkExt;
use futures::stream::StreamExt;
use ipld_core::ipld::Ipld;
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
    let ipld: Ipld = serde_ipld_dagcbor::from_reader(data)
        .map_err(|e| WintermuteError::Serialization(format!("failed to parse ipld: {e}")))?;

    let Ok(Some(header)) = ipld.get("t") else {
        return Ok(None);
    };

    let header_str = match header {
        Ipld::String(s) => s.as_str(),
        _ => return Ok(None),
    };

    if header_str != "#labels" {
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

    // Parse labels array
    let labels_ipld = body
        .get("labels")
        .ok()
        .flatten()
        .ok_or_else(|| WintermuteError::Serialization("missing labels field".into()))?;

    let Ipld::List(labels_list) = labels_ipld else {
        return Err(WintermuteError::Serialization(
            "labels is not a list".into(),
        ));
    };

    let mut labels = Vec::new();
    for label_ipld in labels_list {
        if let Some(label) = parse_label(label_ipld)? {
            labels.push(label);
        }
    }

    Ok(Some(LabelEvent { seq, labels }))
}

fn parse_label(ipld: &Ipld) -> Result<Option<Label>, WintermuteError> {
    let src = ipld
        .get("src")
        .ok()
        .flatten()
        .and_then(|v| match v {
            Ipld::String(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| WintermuteError::Serialization("missing src in label".into()))?;

    let uri = ipld
        .get("uri")
        .ok()
        .flatten()
        .and_then(|v| match v {
            Ipld::String(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| WintermuteError::Serialization("missing uri in label".into()))?;

    let val = ipld
        .get("val")
        .ok()
        .flatten()
        .and_then(|v| match v {
            Ipld::String(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| WintermuteError::Serialization("missing val in label".into()))?;

    let cts = ipld
        .get("cts")
        .ok()
        .flatten()
        .and_then(|v| match v {
            Ipld::String(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| WintermuteError::Serialization("missing cts in label".into()))?;

    Ok(Some(Label { src, uri, val, cts }))
}
