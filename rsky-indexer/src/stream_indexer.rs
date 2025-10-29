use crate::consumer::{RedisConsumer, StreamMessage};
use crate::indexing::{IndexingOptions, IndexingService, WriteOpAction};
use crate::{IndexerConfig, IndexerError, IndexerMetrics, StreamEvent, SEQ_BACKFILL};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, error, info};

/// StreamIndexer reads from Redis streams and indexes events into PostgreSQL
pub struct StreamIndexer {
    consumer: RedisConsumer,
    indexing_service: Arc<IndexingService>,
    config: IndexerConfig,
    #[allow(dead_code)]
    metrics: Arc<IndexerMetrics>,
    shutdown: Arc<AtomicBool>,
    semaphore: Arc<Semaphore>,
}

impl StreamIndexer {
    pub async fn new(
        config: IndexerConfig,
        indexing_service: Arc<IndexingService>,
    ) -> Result<Self, IndexerError> {
        // For now, use the first stream
        let stream = config.streams.first().cloned().unwrap_or_default();

        let consumer = RedisConsumer::new(
            config.redis_url.clone(),
            stream,
            config.consumer_group.clone(),
            config.consumer_name.clone(),
        )
        .await?;

        let semaphore = Arc::new(Semaphore::new(config.concurrency));

        Ok(Self {
            consumer,
            indexing_service,
            config,
            metrics: Arc::new(IndexerMetrics::default()),
            shutdown: Arc::new(AtomicBool::new(false)),
            semaphore,
        })
    }

    /// Run the stream indexer
    pub async fn run(&self) -> Result<(), IndexerError> {
        info!(
            "Starting StreamIndexer for stream: {:?}",
            self.config.streams
        );

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
                    info!("Stream deleted, shutting down");
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
                let permit = self.semaphore.clone().acquire_owned().await.unwrap();
                let indexing_service = self.indexing_service.clone();
                let consumer = self.consumer.clone();

                let handle = tokio::spawn(async move {
                    let result = Self::handle_message(message.clone(), &indexing_service).await;

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
                            error!("Failed to process message {}: {:?}", message.id, e);
                        }
                    }

                    drop(permit);
                });

                handles.push(handle);
            }

            // Wait for all messages to be processed before continuing
            // This ensures messages are ACKed before shutdown
            for handle in handles {
                let _ = handle.await;
            }
        }

        info!("StreamIndexer stopped");
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
                        "Retrying ACK for message {} (attempt {}/{})",
                        message_id, retries, max_retries
                    );
                }
            }
        }
    }

    /// Handle a single message
    async fn handle_message(
        message: StreamMessage,
        indexing_service: &IndexingService,
    ) -> Result<(), IndexerError> {
        // Parse the event from the message
        let event_json = message
            .contents
            .get("event")
            .ok_or_else(|| IndexerError::Serialization("Missing event field".to_string()))?;

        let event: StreamEvent = serde_json::from_str(event_json)
            .map_err(|e| IndexerError::Serialization(e.to_string()))?;

        Self::process_event(event, indexing_service).await
    }

    /// Process a single stream event
    async fn process_event(
        event: StreamEvent,
        indexing_service: &IndexingService,
    ) -> Result<(), IndexerError> {
        match event {
            StreamEvent::Create {
                did,
                collection,
                rkey,
                cid,
                record,
                time,
                rev,
                commit,
                seq,
            } => {
                let uri = format!("at://{}/{}/{}", did, collection, rkey);
                indexing_service
                    .index_record(
                        &uri,
                        &cid,
                        &record,
                        WriteOpAction::Create,
                        &time,
                        &rev,
                        IndexingOptions {
                            disable_notifs: true,
                        },
                    )
                    .await?;

                if seq != SEQ_BACKFILL {
                    indexing_service
                        .set_commit_last_seen(&did, &commit, &rev)
                        .await?;
                }
            }
            StreamEvent::Update {
                did,
                collection,
                rkey,
                cid,
                record,
                time,
                rev,
                commit,
                seq,
            } => {
                let uri = format!("at://{}/{}/{}", did, collection, rkey);
                indexing_service
                    .index_record(
                        &uri,
                        &cid,
                        &record,
                        WriteOpAction::Update,
                        &time,
                        &rev,
                        IndexingOptions {
                            disable_notifs: true,
                        },
                    )
                    .await?;

                if seq != SEQ_BACKFILL {
                    indexing_service
                        .set_commit_last_seen(&did, &commit, &rev)
                        .await?;
                }
            }
            StreamEvent::Delete {
                did,
                collection,
                rkey,
                rev,
                commit,
                seq,
                time: _,
            } => {
                let uri = format!("at://{}/{}/{}", did, collection, rkey);
                indexing_service.delete_record(&uri, &rev).await?;

                if seq != SEQ_BACKFILL {
                    indexing_service
                        .set_commit_last_seen(&did, &commit, &rev)
                        .await?;
                }
            }
            StreamEvent::Repo {
                did, commit, rev, ..
            } => {
                indexing_service
                    .set_commit_last_seen(&did, &commit, &rev)
                    .await?;
            }
            StreamEvent::Account {
                did,
                active,
                status,
                time,
                ..
            } => {
                // Handle "deleted" status specially - delete the actor entirely
                // Case-insensitive comparison since statuses may come capitalized
                if !active && status.as_ref().map(|s| s.to_lowercase()).as_deref() == Some("deleted") {
                    indexing_service.delete_actor(&did).await?;
                } else {
                    indexing_service
                        .update_actor_status(&did, active, status)
                        .await?;
                    indexing_service.index_handle(&did, &time).await?;
                }
            }
            StreamEvent::Identity { did, time, .. } => {
                indexing_service.index_handle(&did, &time).await?;
            }
        }

        Ok(())
    }

    /// Shutdown the indexer
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}
