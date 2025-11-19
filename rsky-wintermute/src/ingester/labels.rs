use crate::SHUTDOWN;
use crate::storage::Storage;
use crate::types::WintermuteError;
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
            if let Some(seq) = parse_label_message(&data)? {
                storage.set_cursor(&cursor_key, seq)?;
            }
        }
    }

    ping_task.abort();
    Ok(())
}

fn parse_label_message(data: &[u8]) -> Result<Option<i64>, WintermuteError> {
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

    Ok(Some(seq))
}
