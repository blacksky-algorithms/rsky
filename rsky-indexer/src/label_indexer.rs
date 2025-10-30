use crate::consumer::{RedisConsumer, StreamMessage};
use crate::{IndexerConfig, IndexerError, IndexerMetrics, Label, LabelStreamEvent};
use deadpool_postgres::Pool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, error, info};

/// LabelIndexer reads from the label_live Redis stream and indexes labels into PostgreSQL
pub struct LabelIndexer {
    consumer: RedisConsumer,
    pool: Pool,
    config: IndexerConfig,
    #[allow(dead_code)]
    metrics: Arc<IndexerMetrics>,
    shutdown: Arc<AtomicBool>,
    semaphore: Arc<Semaphore>,
}

impl LabelIndexer {
    pub async fn new(config: IndexerConfig, pool: Pool) -> Result<Self, IndexerError> {
        let consumer = RedisConsumer::new(
            config.redis_url.clone(),
            "label_live".to_string(),
            config.consumer_group.clone(),
            config.consumer_name.clone(),
        )
        .await?;

        let semaphore = Arc::new(Semaphore::new(config.concurrency));

        Ok(Self {
            consumer,
            pool,
            config,
            metrics: Arc::new(IndexerMetrics::default()),
            shutdown: Arc::new(AtomicBool::new(false)),
            semaphore,
        })
    }

    /// Run the label indexer
    pub async fn run(&self) -> Result<(), IndexerError> {
        info!("Starting LabelIndexer");

        // Ensure consumer group exists
        self.consumer.ensure_consumer_group().await?;

        let mut cursor = "0".to_string(); // Start with pending messages

        while !self.shutdown.load(Ordering::Relaxed) {
            // Read messages from Redis
            let messages = match self
                .consumer
                .read_messages(&cursor, self.config.batch_size)
                .await
            {
                Ok(messages) => messages,
                Err(IndexerError::Redis(e)) if e.to_string().contains("NOGROUP") => {
                    // Stream or consumer group was deleted - clean shutdown
                    info!("Label stream deleted, shutting down");
                    break;
                }
                Err(e) => return Err(e),
            };

            if messages.is_empty() {
                // Switch to live stream after processing pending
                if cursor == "0" {
                    cursor = ">".to_string();
                    info!("Switched to live stream");
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                continue;
            }

            // Update cursor for next iteration
            if cursor != ">" {
                if let Some(last_msg) = messages.last() {
                    cursor = last_msg.id.clone();
                }
            }

            // Process messages concurrently
            let mut handles = Vec::new();

            for message in messages {
                // Acquire semaphore permit - if this fails, the semaphore was closed
                let permit = match self.semaphore.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(e) => {
                        error!(
                            "Failed to acquire semaphore permit: {:?}, skipping message",
                            e
                        );
                        continue;
                    }
                };
                let pool = self.pool.clone();
                let consumer = self.consumer.clone();

                let handle = tokio::spawn(async move {
                    let result = Self::handle_message(message.clone(), &pool).await;

                    match result {
                        Ok(_) => {
                            // ACK and delete the message with retry
                            if let Err(e) = Self::ack_with_retry(&consumer, &message.id, 3).await {
                                error!(
                                    "Failed to ACK message {} after retries: {:?}",
                                    message.id, e
                                );
                            }
                        }
                        Err(e) => {
                            error!("Failed to process label message {}: {:?}", message.id, e);
                        }
                    }

                    drop(permit);
                });

                handles.push(handle);
            }

            // Wait for all messages to be processed
            for handle in handles {
                let _ = handle.await;
            }
        }

        info!("LabelIndexer stopped");
        Ok(())
    }

    /// ACK a message with retry logic for transient failures
    async fn ack_with_retry(
        consumer: &RedisConsumer,
        message_id: &str,
        max_retries: u32,
    ) -> Result<(), IndexerError> {
        let mut retries = 0;
        let mut delay_ms = 10; // Start with 10ms

        loop {
            match consumer.ack_message(message_id, true).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    retries += 1;
                    if retries >= max_retries {
                        return Err(e);
                    }

                    // Exponential backoff: 10ms, 20ms, 40ms, etc.
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    delay_ms *= 2;

                    debug!(
                        "Retrying ACK for label message {} (attempt {}/{})",
                        message_id, retries, max_retries
                    );
                }
            }
        }
    }

    /// Handle a single message
    async fn handle_message(message: StreamMessage, pool: &Pool) -> Result<(), IndexerError> {
        // Parse the label event from the message
        let labels_json = message
            .contents
            .get("labels")
            .ok_or_else(|| IndexerError::Serialization("Missing labels field".to_string()))?;

        let event: LabelStreamEvent = serde_json::from_str(labels_json)
            .map_err(|e| IndexerError::Serialization(e.to_string()))?;

        Self::process_labels(event.labels, pool).await
    }

    /// Process and index labels
    async fn process_labels(labels: Vec<Label>, pool: &Pool) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        for label in labels {
            // Check if neg (negation) - if true, delete the label
            if label.neg.unwrap_or(false) {
                client
                    .execute(
                        "DELETE FROM label WHERE src = $1 AND uri = $2 AND val = $3",
                        &[&label.src, &label.uri, &label.val],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;

                debug!("Deleted label: {} on {}", label.val, label.uri);
            } else {
                // Insert or update the label
                client
                    .execute(
                        r#"
                        INSERT INTO label (src, uri, cid, val, cts, exp)
                        VALUES ($1, $2, $3, $4, $5, NULL)
                        ON CONFLICT (src, uri, cid, val) DO UPDATE
                        SET cid = EXCLUDED.cid,
                            cts = EXCLUDED.cts,
                            exp = EXCLUDED.exp
                        "#,
                        &[
                            &label.src,
                            &label.uri,
                            &label.cid.as_deref(),
                            &label.val,
                            &label.cts,
                        ],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;

                debug!("Indexed label: {} on {}", label.val, label.uri);
            }
        }

        Ok(())
    }

    /// Shutdown the indexer
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}
