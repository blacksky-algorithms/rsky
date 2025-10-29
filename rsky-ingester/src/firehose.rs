use crate::batcher::Batcher;
use crate::{streams, IngesterConfig, IngesterError, StreamEvent};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use ipld_core::ipld::Ipld;
use iroh_car::CarReader;
use lexicon_cid::Cid;
use redis::AsyncCommands;
use rsky_lexicon::com::atproto::sync::SubscribeRepos;
use std::io::Cursor;
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

/// FirehoseIngester subscribes to com.atproto.sync.subscribeRepos
/// and writes events to the firehose_live Redis stream
pub struct FirehoseIngester {
    config: IngesterConfig,
    redis_client: redis::Client,
}

impl FirehoseIngester {
    pub fn new(config: IngesterConfig) -> Result<Self, IngesterError> {
        let redis_client = redis::Client::open(config.redis_url.clone())?;
        Ok(Self {
            config,
            redis_client,
        })
    }

    pub async fn run(&self, hostname: String) -> Result<(), IngesterError> {
        info!("Starting FirehoseIngester for {}", hostname);

        loop {
            if let Err(e) = self.run_connection(&hostname).await {
                error!("FirehoseIngester error for {}: {:?}", hostname, e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    async fn run_connection(&self, hostname: &str) -> Result<(), IngesterError> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        // Get cursor from Redis
        let cursor_key = format!("{}:cursor:{}", streams::FIREHOSE_LIVE, hostname);
        let cursor: Option<i64> = conn.get(&cursor_key).await.unwrap_or(None);

        // Strip protocol from hostname if present (e.g., "https://example.com" -> "example.com")
        let clean_hostname = hostname
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');

        // Build WebSocket URL
        let mut url = url::Url::parse(&format!(
            "wss://{}/xrpc/com.atproto.sync.subscribeRepos",
            clean_hostname
        ))
        .map_err(|e| IngesterError::Other(e.into()))?;

        // Cursor semantics:
        // - cursor=0: Default for new subscribers, get everything from beginning
        // - cursor=N: Resume from last seen sequence (N is large monotonic number)
        // - No cursor: Live events only (can be enabled via env var if needed)
        let cursor_value = cursor.unwrap_or(0);
        url.query_pairs_mut()
            .append_pair("cursor", &cursor_value.to_string());

        info!("Connecting to {} with cursor {}", url, cursor_value);

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
                let stream_len: usize = conn.xlen(streams::FIREHOSE_LIVE).await?;
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
                    Ok(events) => {
                        for event in events {
                            if let Err(e) = batch_tx.send(event) {
                                error!("Failed to send event to batcher: {:?}", e);
                                break;
                            }
                        }
                    }
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

    async fn process_message(&self, data: &[u8]) -> Result<Vec<StreamEvent>, IngesterError> {
        let result = rsky_firehose::firehose::read(data)
            .map_err(|e| IngesterError::Serialization(format!("{:?}", e)))?;

        // Skip if message was filtered (e.g., #sync, #info messages)
        let Some((header, body)) = result else {
            return Ok(Vec::new());
        };

        let mut events = Vec::new();
        let time = Utc::now().to_rfc3339();

        match body {
            SubscribeRepos::Commit(commit) => {
                let seq = header.operation as i64;
                let did = commit.repo.clone();
                let commit_cid = commit.commit.to_string();
                let rev = commit.rev.clone();

                // Process operations from the commit
                for op in commit.ops {
                    let collection = op.path.split('/').next().unwrap_or("").to_string();
                    let rkey = op.path.split('/').nth(1).unwrap_or("").to_string();

                    if collection.is_empty() || rkey.is_empty() {
                        continue;
                    }

                    match op.action.as_str() {
                        "create" => {
                            if let Some(cid) = op.cid {
                                // Try to find the record in the blocks
                                let record = self
                                    .extract_record_from_blocks(&commit.blocks, &cid)
                                    .await?;

                                events.push(StreamEvent::Create {
                                    seq,
                                    time: time.clone(),
                                    did: did.clone(),
                                    commit: commit_cid.clone(),
                                    rev: rev.clone(),
                                    collection: collection.clone(),
                                    rkey: rkey.clone(),
                                    cid: cid.to_string(),
                                    record,
                                });
                            }
                        }
                        "update" => {
                            if let Some(cid) = op.cid {
                                let record = self
                                    .extract_record_from_blocks(&commit.blocks, &cid)
                                    .await?;

                                events.push(StreamEvent::Update {
                                    seq,
                                    time: time.clone(),
                                    did: did.clone(),
                                    commit: commit_cid.clone(),
                                    rev: rev.clone(),
                                    collection: collection.clone(),
                                    rkey: rkey.clone(),
                                    cid: cid.to_string(),
                                    record,
                                });
                            }
                        }
                        "delete" => {
                            events.push(StreamEvent::Delete {
                                seq,
                                time: time.clone(),
                                did: did.clone(),
                                commit: commit_cid.clone(),
                                rev: rev.clone(),
                                collection: collection.clone(),
                                rkey: rkey.clone(),
                            });
                        }
                        _ => {}
                    }
                }

                // Add repo sync event at the end
                events.push(StreamEvent::Repo {
                    seq,
                    time: time.clone(),
                    did,
                    commit: commit_cid,
                    rev,
                });
            }
            SubscribeRepos::Handle(handle_evt) => {
                events.push(StreamEvent::Identity {
                    seq: header.operation as i64,
                    time,
                    did: handle_evt.did,
                    handle: handle_evt.handle,
                });
            }
            SubscribeRepos::Account(account_evt) => {
                events.push(StreamEvent::Account {
                    seq: header.operation as i64,
                    time,
                    did: account_evt.did,
                    active: account_evt.active,
                    status: account_evt.status.map(|s| format!("{:?}", s)),
                });
            }
            SubscribeRepos::Identity(identity_evt) => {
                if let Some(handle) = identity_evt.handle {
                    events.push(StreamEvent::Identity {
                        seq: header.operation as i64,
                        time,
                        did: identity_evt.did,
                        handle,
                    });
                }
            }
            SubscribeRepos::Tombstone(tombstone_evt) => {
                info!("Received tombstone for {}", tombstone_evt.did);
            }
        }

        Ok(events)
    }

    async fn extract_record_from_blocks(
        &self,
        blocks: &[u8],
        target_cid: &Cid,
    ) -> Result<serde_json::Value, IngesterError> {
        // Parse CAR file from bytes
        let cursor = Cursor::new(blocks);
        let mut car_reader = CarReader::new(cursor)
            .await
            .map_err(|e| IngesterError::Serialization(format!("Failed to parse CAR: {:?}", e)))?;

        // Iterate through blocks to find the one matching target_cid
        loop {
            let block_option = car_reader.next_block().await.map_err(|e| {
                IngesterError::Serialization(format!("Failed to read CAR block: {:?}", e))
            })?;

            match block_option {
                Some((cid, data)) => {
                    if cid == *target_cid {
                        // Decode as IPLD CBOR first (handles CIDs properly)
                        let ipld: Ipld = serde_ipld_dagcbor::from_slice(&data).map_err(|e| {
                            IngesterError::Serialization(format!("Failed to decode CBOR: {:?}", e))
                        })?;

                        // Convert IPLD to JSON (CIDs become strings)
                        let json = Self::ipld_to_json(&ipld)?;
                        return Ok(json);
                    }
                }
                None => break,
            }
        }

        // Block not found
        Err(IngesterError::Serialization(format!(
            "Block with CID {} not found in CAR",
            target_cid
        )))
    }

    fn ipld_to_json(ipld: &Ipld) -> Result<serde_json::Value, IngesterError> {
        use serde_json::json;

        match ipld {
            Ipld::Null => Ok(serde_json::Value::Null),
            Ipld::Bool(b) => Ok(json!(b)),
            Ipld::Integer(i) => Ok(json!(i)),
            Ipld::Float(f) => Ok(json!(f)),
            Ipld::String(s) => Ok(json!(s)),
            Ipld::Bytes(b) => Ok(json!(b)),
            Ipld::List(list) => {
                let arr: Result<Vec<_>, _> = list.iter().map(Self::ipld_to_json).collect();
                Ok(serde_json::Value::Array(arr?))
            }
            Ipld::Map(map) => {
                let mut obj = serde_json::Map::new();
                for (k, v) in map.iter() {
                    obj.insert(k.clone(), Self::ipld_to_json(v)?);
                }
                Ok(serde_json::Value::Object(obj))
            }
            Ipld::Link(cid) => {
                // Convert CID to string representation
                Ok(json!(cid.to_string()))
            }
        }
    }

    async fn write_batch(
        conn: &mut redis::aio::MultiplexedConnection,
        batch: &[StreamEvent],
        hostname: &str,
    ) -> Result<(), IngesterError> {
        let mut pipe = redis::pipe();
        pipe.atomic();

        let mut max_seq = 0i64;

        for event in batch {
            let seq = match event {
                StreamEvent::Create { seq, .. }
                | StreamEvent::Update { seq, .. }
                | StreamEvent::Delete { seq, .. }
                | StreamEvent::Repo { seq, .. }
                | StreamEvent::Account { seq, .. }
                | StreamEvent::Identity { seq, .. } => *seq,
            };

            if seq > max_seq {
                max_seq = seq;
            }

            let event_json = serde_json::to_string(event)
                .map_err(|e| IngesterError::Serialization(e.to_string()))?;

            pipe.xadd(streams::FIREHOSE_LIVE, "*", &[("event", event_json)]);
        }

        // Update cursor
        let cursor_key = format!("{}:cursor:{}", streams::FIREHOSE_LIVE, hostname);
        pipe.set(&cursor_key, max_seq);

        pipe.query_async::<()>(conn).await?;

        Ok(())
    }
}
