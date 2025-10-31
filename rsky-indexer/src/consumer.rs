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

        info!(
            "XREADGROUP calling: stream={}, group={}, consumer={}, cursor={}, count={}",
            self.stream, self.group, self.consumer, cursor, count
        );

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

        info!(
            "XREADGROUP returned {} messages from stream {}",
            messages.len(),
            self.stream
        );

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

    /// Trim the stream to remove all messages before the given cursor
    /// This frees Redis memory by deleting processed messages
    /// Uses XTRIM with MINID strategy to keep only messages >= cursor
    pub async fn trim_stream(&self, cursor: &str) -> Result<u64, IndexerError> {
        let mut conn = self.manager.clone();

        // XTRIM stream MINID cursor
        let trimmed: u64 = redis::cmd("XTRIM")
            .arg(&self.stream)
            .arg("MINID")
            .arg(cursor)
            .query_async(&mut conn)
            .await?;

        if trimmed > 0 {
            info!(
                "Trimmed {} messages from stream {} (cursor: {})",
                trimmed, self.stream, cursor
            );
        }

        Ok(trimmed)
    }

    /// Get the consumer group's last-delivered-id
    /// This is the safest cursor to use for trimming
    pub async fn get_group_cursor(&self) -> Result<Option<String>, IndexerError> {
        let mut conn = self.manager.clone();

        // XINFO GROUPS stream
        let result: redis::Value = redis::cmd("XINFO")
            .arg("GROUPS")
            .arg(&self.stream)
            .query_async(&mut conn)
            .await?;

        // Parse the result to find our group's last-delivered-id
        if let redis::Value::Array(groups) = result {
            for group_info in groups {
                if let redis::Value::Array(fields) = group_info {
                    let mut group_name: Option<String> = None;
                    let mut last_delivered: Option<String> = None;

                    // Fields come in pairs: [key, value, key, value, ...]
                    for i in (0..fields.len()).step_by(2) {
                        if let (Some(redis::Value::BulkString(k)), Some(v)) =
                            (fields.get(i), fields.get(i + 1))
                        {
                            let key = String::from_utf8_lossy(k);
                            if key == "name" {
                                if let redis::Value::BulkString(name) = v {
                                    group_name = Some(String::from_utf8_lossy(name).to_string());
                                }
                            } else if key == "last-delivered-id" {
                                if let redis::Value::BulkString(id) = v {
                                    last_delivered = Some(String::from_utf8_lossy(id).to_string());
                                }
                            }
                        }
                    }

                    // If this is our group, return its last-delivered-id
                    if group_name.as_deref() == Some(&self.group) {
                        return Ok(last_delivered);
                    }
                }
            }
        }

        Ok(None)
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
