use crate::consumer::{RedisConsumer, StreamMessage};
use crate::indexing::{IndexingOptions, IndexingService, WriteOpAction};
use crate::{metrics, IndexerConfig, IndexerError, StreamEvent, SEQ_BACKFILL};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// StreamIndexer reads from Redis streams and indexes events into PostgreSQL
pub struct StreamIndexer {
    consumer: RedisConsumer,
    indexing_service: Arc<IndexingService>,
    config: IndexerConfig,
    cancellation_token: CancellationToken,
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
            cancellation_token: CancellationToken::new(),
            semaphore,
        })
    }

    /// Initiate graceful shutdown of the stream indexer
    pub fn shutdown(&self) {
        info!("Graceful shutdown requested for StreamIndexer");
        self.cancellation_token.cancel();
    }

    /// Get a clone of the cancellation token for use in external shutdown coordination
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
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

        loop {
            // Check for shutdown signal
            tokio::select! {
                _ = self.cancellation_token.cancelled() => {
                    info!("Shutdown signal received, draining in-flight messages...");
                    break;
                }
                result = self.read_and_process_batch(&mut cursor) => {
                    match result {
                        Ok(should_continue) => {
                            if !should_continue {
                                // Stream deleted or error occurred
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Error processing batch: {:?}", e);
                            return Err(e);
                        }
                    }
                }
            }
        }

        // Wait for all in-flight tasks to complete before shutdown
        info!("Waiting for all in-flight tasks to complete...");
        // Acquire all permits to ensure all tasks are done
        let semaphore = self.semaphore.clone();
        let _all_permits = semaphore.acquire_many(self.config.concurrency as u32).await;

        info!("StreamIndexer stopped gracefully");
        Ok(())
    }

    /// Read and process a batch of messages
    /// Returns Ok(true) to continue, Ok(false) to stop
    async fn read_and_process_batch(&self, cursor: &mut String) -> Result<bool, IndexerError> {
        // Read messages from Redis
        let messages = match self
            .consumer
            .read_messages(cursor, self.config.batch_size)
            .await
        {
            Ok(messages) => messages,
            Err(IndexerError::Redis(e)) if e.to_string().contains("NOGROUP") => {
                // Stream or consumer group was deleted - clean shutdown
                info!("Stream deleted, shutting down");
                return Ok(false);
            }
            Err(e) => return Err(e),
        };

        if messages.is_empty() {
            // Switch to live stream after processing pending
            if *cursor == "0" {
                *cursor = ">".to_string();
                info!("Switched to live stream");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            return Ok(true);
        }

        // Update cursor for next iteration
        if *cursor != ">" {
            if let Some(last_msg) = messages.last() {
                *cursor = last_msg.id.clone();
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
            let indexing_service = self.indexing_service.clone();
            let consumer = self.consumer.clone();

            let handle = tokio::spawn(async move {
                // Extract event info for better error logging
                let event_info = message
                    .contents
                    .get("event")
                    .and_then(|e| serde_json::from_str::<serde_json::Value>(e).ok())
                    .and_then(|v| {
                        let event_type =
                            v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown");
                        let collection = v
                            .get("collection")
                            .and_then(|c| c.as_str())
                            .unwrap_or("unknown");
                        let did = v.get("did").and_then(|d| d.as_str()).unwrap_or("unknown");
                        Some(format!(
                            "type={}, collection={}, did={}",
                            event_type, collection, did
                        ))
                    })
                    .unwrap_or_else(|| "unknown event".to_string());

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
                        // Log expected errors (duplicates, invalid UTF-8) at WARN level
                        // to reduce noise in production logs
                        if e.is_expected_error() {
                            warn!("Skipping message {} [{}]: {}", message.id, event_info, e);
                            metrics::EXPECTED_ERRORS_TOTAL.inc();
                        } else {
                            error!(
                                "Failed to process message {} [{}]: {:?}",
                                message.id, event_info, e
                            );
                            metrics::UNEXPECTED_ERRORS_TOTAL.inc();
                        }
                    }
                }

                drop(permit);
            });

            handles.push(handle);
        }

        // Wait for all messages in this batch to be processed before continuing
        // This ensures messages are ACKed before moving to next batch or shutdown
        for handle in handles {
            let _ = handle.await;
        }

        Ok(true)
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
                    metrics::DB_WRITES_TOTAL.inc(); // commit tracking write
                }

                // Increment metrics
                metrics::EVENTS_PROCESSED_TOTAL.inc();
                metrics::CREATE_EVENTS_TOTAL.inc();
                metrics::DB_WRITES_TOTAL.inc(); // index_record writes

                // Track collection-specific metrics
                if collection == "app.bsky.feed.post" {
                    metrics::POST_EVENTS_TOTAL.inc();
                } else if collection == "app.bsky.feed.like" {
                    metrics::LIKE_EVENTS_TOTAL.inc();
                } else if collection == "app.bsky.feed.repost" {
                    metrics::REPOST_EVENTS_TOTAL.inc();
                } else if collection == "app.bsky.graph.follow" {
                    metrics::FOLLOW_EVENTS_TOTAL.inc();
                } else if collection == "app.bsky.graph.block" {
                    metrics::BLOCK_EVENTS_TOTAL.inc();
                } else if collection == "app.bsky.actor.profile" {
                    metrics::PROFILE_EVENTS_TOTAL.inc();
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
                    metrics::DB_WRITES_TOTAL.inc(); // commit tracking write
                }

                // Increment metrics
                metrics::EVENTS_PROCESSED_TOTAL.inc();
                metrics::UPDATE_EVENTS_TOTAL.inc();
                metrics::DB_WRITES_TOTAL.inc(); // index_record writes
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
                    metrics::DB_WRITES_TOTAL.inc(); // commit tracking write
                }

                // Increment metrics
                metrics::EVENTS_PROCESSED_TOTAL.inc();
                metrics::DELETE_EVENTS_TOTAL.inc();
                metrics::DB_WRITES_TOTAL.inc(); // delete_record writes
            }
            StreamEvent::Repo {
                did, commit, rev, ..
            } => {
                indexing_service
                    .set_commit_last_seen(&did, &commit, &rev)
                    .await?;
                metrics::DB_WRITES_TOTAL.inc(); // commit tracking write
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
                if !active
                    && status.as_ref().map(|s| s.to_lowercase()).as_deref() == Some("deleted")
                {
                    indexing_service.delete_actor(&did).await?;
                } else {
                    indexing_service
                        .update_actor_status(&did, active, status)
                        .await?;
                    indexing_service.index_handle(&did, &time).await?;
                }
                metrics::DB_WRITES_TOTAL.inc(); // actor status/handle writes
            }
            StreamEvent::Identity { did, time, .. } => {
                indexing_service.index_handle(&did, &time).await?;
                metrics::DB_WRITES_TOTAL.inc(); // handle index write
            }
        }

        Ok(())
    }
}
