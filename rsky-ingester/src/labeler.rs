use crate::batcher::Batcher;
use crate::{streams, IngesterConfig, IngesterError, LabelStreamEvent};
use futures_util::{SinkExt, StreamExt};
use redis::AsyncCommands;
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

/// LabelerIngester subscribes to com.atproto.label.subscribeLabels
/// and writes label events to the label_live Redis stream
pub struct LabelerIngester {
    config: IngesterConfig,
    redis_client: redis::Client,
}

impl LabelerIngester {
    pub fn new(config: IngesterConfig) -> Result<Self, IngesterError> {
        let redis_client = redis::Client::open(config.redis_url.clone())?;
        Ok(Self {
            config,
            redis_client,
        })
    }

    pub async fn run(&self, hostname: String) -> Result<(), IngesterError> {
        info!("Starting LabelerIngester for {}", hostname);

        loop {
            if let Err(e) = self.run_connection(&hostname).await {
                error!("LabelerIngester error for {}: {:?}", hostname, e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    async fn run_connection(&self, hostname: &str) -> Result<(), IngesterError> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        // Get cursor from Redis
        let cursor_key = format!("{}:cursor:{}", streams::LABEL_LIVE, hostname);
        let cursor: Option<i64> = conn.get(&cursor_key).await.unwrap_or(None);

        // Build WebSocket URL
        let mut url = url::Url::parse(&format!(
            "wss://{}/xrpc/com.atproto.label.subscribeLabels",
            hostname
        ))
        .map_err(|e| IngesterError::Other(e.into()))?;

        // Start from cursor 0 if none exists
        let cursor = cursor.unwrap_or(0);
        url.query_pairs_mut()
            .append_pair("cursor", &cursor.to_string());

        info!("Connecting to {} with cursor {}", url, cursor);

        let (ws_stream, _) = connect_async(url.as_str()).await?;
        let (mut write, mut read) = ws_stream.split();

        // Create batcher for events
        let (batch_tx, mut batch_rx) =
            Batcher::new(self.config.batch_size, self.config.batch_timeout_ms);

        // Spawn task to handle batched writes to Redis
        let redis_client = self.redis_client.clone();
        let hostname_clone = hostname.to_string();
        let high_water_mark = self.config.high_water_mark;
        let write_task = tokio::spawn(async move {
            let mut conn = redis_client.get_multiplexed_async_connection().await?;

            while let Some(batch) = batch_rx.recv().await {
                // Check backpressure
                let stream_len: usize = conn.xlen(streams::LABEL_LIVE).await?;
                if stream_len >= high_water_mark {
                    warn!(
                        "Backpressure: stream length {} >= {}",
                        stream_len, high_water_mark
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }

                // Write batch to Redis stream
                Self::write_batch(&mut conn, &batch, &hostname_clone).await?;
            }

            Ok::<_, IngesterError>(())
        });

        // Spawn task to send periodic pings
        let ping_task = tokio::spawn(async move {
            let mut ping_interval = interval(Duration::from_secs(30));
            loop {
                ping_interval.tick().await;
                if let Err(e) = write.send(Message::Ping(vec![])).await {
                    error!("Failed to send ping: {:?}", e);
                    break;
                }
            }
        });

        // Read messages from WebSocket
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Binary(data)) => match self.process_message(&data).await {
                    Ok(Some(event)) => {
                        if let Err(e) = batch_tx.send(event) {
                            error!("Failed to send event to batcher: {:?}", e);
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        error!("Failed to process message: {:?}", e);
                    }
                },
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    info!("WebSocket closed: {:?}", frame);
                    break;
                }
                Ok(msg) => {
                    warn!("Unexpected message type: {:?}", msg);
                }
                Err(e) => {
                    error!("WebSocket error: {:?}", e);
                    break;
                }
            }
        }

        // Cleanup
        drop(batch_tx);
        write_task.abort();
        ping_task.abort();

        Ok(())
    }

    async fn process_message(
        &self,
        data: &[u8],
    ) -> Result<Option<LabelStreamEvent>, IngesterError> {
        let (header, body) = rsky_firehose::firehose::read_labels(data)
            .map_err(|e| IngesterError::Serialization(format!("{:?}", e)))?;

        let seq = header.operation as i64;

        // Convert SubscribeLabels to LabelStreamEvent
        let labels = body
            .labels
            .into_iter()
            .map(|l| crate::Label {
                src: l.src,
                uri: l.uri,
                cid: l.cid.map(|c| c.to_string()),
                val: l.val,
                neg: l.neg,
                cts: l.cts.to_string(),
            })
            .collect();

        Ok(Some(LabelStreamEvent { seq, labels }))
    }

    async fn write_batch(
        conn: &mut redis::aio::MultiplexedConnection,
        batch: &[LabelStreamEvent],
        hostname: &str,
    ) -> Result<(), IngesterError> {
        let mut pipe = redis::pipe();
        pipe.atomic();

        let mut max_seq = 0i64;

        for event in batch {
            if event.seq > max_seq {
                max_seq = event.seq;
            }

            let event_json = serde_json::to_string(event)
                .map_err(|e| IngesterError::Serialization(e.to_string()))?;

            pipe.xadd(streams::LABEL_LIVE, "*", &[("labels", event_json)]);
        }

        // Update cursor
        let cursor_key = format!("{}:cursor:{}", streams::LABEL_LIVE, hostname);
        pipe.set(&cursor_key, max_seq);

        pipe.query_async::<()>(conn).await?;

        Ok(())
    }
}
