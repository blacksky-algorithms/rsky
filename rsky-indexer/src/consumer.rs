use crate::IndexerError;
use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::AsyncCommands;
use std::collections::HashMap;
use tracing::{debug, info};

/// Redis message from a stream
#[derive(Debug, Clone)]
pub struct StreamMessage {
    pub id: String,
    pub contents: HashMap<String, String>,
}

/// Redis consumer for reading from streams with consumer groups
/// Uses connection pooling for production scalability
#[derive(Clone)]
pub struct RedisConsumer {
    manager: redis::aio::ConnectionManager,
    pub stream: String,
    pub group: String,
    pub consumer: String,
}

impl RedisConsumer {
    pub async fn new(
        redis_url: String,
        stream: String,
        group: String,
        consumer: String,
    ) -> Result<Self, IndexerError> {
        let client = redis::Client::open(redis_url)?;
        let manager = redis::aio::ConnectionManager::new(client).await?;
        Ok(Self {
            manager,
            stream,
            group,
            consumer,
        })
    }

    /// Ensure the consumer group exists, create it if not
    pub async fn ensure_consumer_group(&self) -> Result<(), IndexerError> {
        let mut conn = self.manager.clone();

        // Try to create the consumer group
        // XGROUP CREATE stream group id [MKSTREAM]
        let result: Result<String, redis::RedisError> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&self.stream)
            .arg(&self.group)
            .arg("0") // Start from beginning
            .arg("MKSTREAM") // Create stream if it doesn't exist
            .query_async(&mut conn)
            .await;

        match result {
            Ok(_) => {
                info!(
                    "Created consumer group '{}' for stream '{}'",
                    self.group, self.stream
                );
                Ok(())
            }
            Err(e) => {
                // Consumer group may already exist, which is fine
                if e.to_string().contains("BUSYGROUP") {
                    debug!(
                        "Consumer group '{}' already exists for stream '{}'",
                        self.group, self.stream
                    );
                    Ok(())
                } else {
                    Err(IndexerError::Redis(e))
                }
            }
        }
    }

    /// Read messages from the consumer group
    /// cursor: "0" for pending messages, ">" for new messages
    pub async fn read_messages(
        &self,
        cursor: &str,
        count: usize,
    ) -> Result<Vec<StreamMessage>, IndexerError> {
        let mut conn = self.manager.clone();

        let opts = StreamReadOptions::default()
            .group(&self.group, &self.consumer)
            .count(count)
            .block(1000); // Block for 1 second if no messages

        let reply: StreamReadReply = conn
            .xread_options(&[&self.stream], &[cursor], &opts)
            .await?;

        let messages: Vec<StreamMessage> = reply
            .keys
            .into_iter()
            .flat_map(|stream_key| {
                stream_key.ids.into_iter().map(|stream_id| {
                    let contents: HashMap<String, String> = stream_id
                        .map
                        .into_iter()
                        .map(|(k, v)| {
                            let value = match v {
                                redis::Value::BulkString(bytes) => {
                                    String::from_utf8_lossy(&bytes).to_string()
                                }
                                redis::Value::SimpleString(s) => s,
                                _ => format!("{:?}", v),
                            };
                            (k, value)
                        })
                        .collect();

                    StreamMessage {
                        id: stream_id.id,
                        contents,
                    }
                })
            })
            .collect();

        Ok(messages)
    }

    /// Acknowledge a message and optionally delete it from the stream
    pub async fn ack_message(&self, message_id: &str, delete: bool) -> Result<(), IndexerError> {
        let mut conn = self.manager.clone();

        // XACK stream group id
        let _: u64 = conn.xack(&self.stream, &self.group, &[message_id]).await?;

        if delete {
            // XDEL stream id
            let _: u64 = conn.xdel(&self.stream, &[message_id]).await?;
        }

        Ok(())
    }

    /// Get pending messages count
    pub async fn get_pending_count(&self) -> Result<usize, IndexerError> {
        let mut conn = self.manager.clone();

        // XPENDING stream group
        let result: redis::Value = redis::cmd("XPENDING")
            .arg(&self.stream)
            .arg(&self.group)
            .query_async(&mut conn)
            .await?;

        // The result is an array: [count, start_id, end_id, consumers]
        if let redis::Value::Array(arr) = result {
            if let Some(redis::Value::Int(count)) = arr.first() {
                return Ok(*count as usize);
            }
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Redis running
    async fn test_consumer_group() {
        let consumer = RedisConsumer::new(
            "redis://localhost:6379".to_string(),
            "test_stream".to_string(),
            "test_group".to_string(),
            "test_consumer".to_string(),
        )
        .unwrap();

        consumer.ensure_consumer_group().await.unwrap();
    }
}
