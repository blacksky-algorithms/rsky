use crate::{BackfillEvent, BackfillerConfig, BackfillerError, StreamEvent, SEQ_BACKFILL};
use redis::AsyncCommands;
use rsky_repo::block_map::BlockMap;
use rsky_repo::car::read_car_with_root;
use rsky_repo::parse::get_and_parse_record;
use rsky_repo::readable_repo::ReadableRepo;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_repo::util::verify_commit_sig;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, RwLock};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Redis stream message
#[derive(Debug, Clone)]
struct StreamMessage {
    id: String,
    data: BackfillEvent,
}

/// Main repo backfiller
pub struct RepoBackfiller {
    config: BackfillerConfig,
    redis_client: redis::Client,
    semaphore: Arc<Semaphore>,
    http_client: reqwest::Client,
}

impl RepoBackfiller {
    pub fn new(config: BackfillerConfig) -> Result<Self, BackfillerError> {
        let redis_client = redis::Client::open(config.redis_url.as_str())?;
        let semaphore = Arc::new(Semaphore::new(config.concurrency));
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;

        Ok(Self {
            config,
            redis_client,
            semaphore,
            http_client,
        })
    }

    /// Run the backfiller
    pub async fn run(&self) -> Result<(), BackfillerError> {
        info!("Starting repo backfiller");

        // Ensure consumer group exists
        self.ensure_consumer_group().await?;

        // Start processing loop
        self.process_loop().await
    }

    /// Ensure consumer group exists
    async fn ensure_consumer_group(&self) -> Result<(), BackfillerError> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        // Try to create consumer group (ignore error if it already exists)
        let _: Result<String, redis::RedisError> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&self.config.stream_in)
            .arg(&self.config.consumer_group)
            .arg("0")
            .arg("MKSTREAM")
            .query_async(&mut conn)
            .await;

        Ok(())
    }

    /// Main processing loop
    async fn process_loop(&self) -> Result<(), BackfillerError> {
        let mut cursor = "0".to_string();

        loop {
            // Check backpressure on output stream
            self.check_backpressure().await?;

            // Read messages from consumer group
            let messages = self.read_messages(&cursor, 100).await?;

            if messages.is_empty() {
                if cursor == ">" {
                    // No new messages, wait a bit
                    sleep(Duration::from_millis(100)).await;
                } else {
                    // Move to reading new messages
                    cursor = ">".to_string();
                }
                continue;
            }

            // Process messages concurrently
            let mut tasks = Vec::new();
            for msg in &messages {
                let permit = self.semaphore.clone().acquire_owned().await.unwrap();
                let msg_clone = msg.clone();
                let self_clone = self.clone_for_task();

                let task = tokio::spawn(async move {
                    let result = self_clone.handle_message(&msg_clone).await;
                    drop(permit);
                    (msg_clone.id, result)
                });

                tasks.push(task);
            }

            // Wait for all tasks to complete
            for task in tasks {
                let (msg_id, result) = task.await.unwrap();
                match result {
                    Ok(_) => {
                        debug!("Successfully processed message {}", msg_id);
                    }
                    Err(e) => {
                        error!("Failed to process message {}: {:?}", msg_id, e);
                    }
                }
            }

            // Update cursor
            if cursor != ">" {
                cursor = messages.last().map(|m| m.id.clone()).unwrap_or(">".to_string());
            }
        }
    }

    /// Read messages from Redis stream
    async fn read_messages(
        &self,
        cursor: &str,
        count: usize,
    ) -> Result<Vec<StreamMessage>, BackfillerError> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        let results: Vec<redis::Value> = redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(&self.config.consumer_group)
            .arg(&self.config.consumer_name)
            .arg("COUNT")
            .arg(count)
            .arg("STREAMS")
            .arg(&self.config.stream_in)
            .arg(cursor)
            .query_async(&mut conn)
            .await?;

        let mut messages = Vec::new();

        if let Some(redis::Value::Array(streams)) = results.first() {
            if let Some(redis::Value::Array(stream_data)) = streams.get(1) {
                for entry in stream_data {
                    if let redis::Value::Array(entry_data) = entry {
                        if let (Some(redis::Value::BulkString(id)), Some(redis::Value::Array(fields))) =
                            (entry_data.first(), entry_data.get(1))
                        {
                            let id = String::from_utf8_lossy(id).to_string();

                            // Parse fields
                            let mut repo_json = None;
                            for i in (0..fields.len()).step_by(2) {
                                if let (Some(redis::Value::BulkString(key)), Some(redis::Value::BulkString(value))) =
                                    (fields.get(i), fields.get(i + 1))
                                {
                                    let key_str = String::from_utf8_lossy(key);
                                    if key_str == "repo" {
                                        repo_json = Some(String::from_utf8_lossy(value).to_string());
                                    }
                                }
                            }

                            if let Some(json) = repo_json {
                                match serde_json::from_str::<BackfillEvent>(&json) {
                                    Ok(data) => messages.push(StreamMessage { id, data }),
                                    Err(e) => {
                                        error!("Failed to parse backfill event: {:?}", e);
                                        // ACK and delete bad message
                                        self.ack_message(&id, true).await?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(messages)
    }

    /// Check backpressure on output stream
    async fn check_backpressure(&self) -> Result<(), BackfillerError> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        loop {
            let len: usize = conn.xlen(&self.config.stream_out).await?;

            if len < self.config.high_water_mark {
                break;
            }

            warn!(
                "Backpressure: output stream length {} exceeds high water mark {}",
                len, self.config.high_water_mark
            );
            sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    /// Handle a single backfill message
    async fn handle_message(&self, msg: &StreamMessage) -> Result<(), BackfillerError> {
        info!("Processing repo backfill for DID: {}", msg.data.did);

        // Fetch repo CAR
        let car_bytes = self.fetch_repo(&msg.data.host, &msg.data.did).await?;

        // Parse CAR
        let car = read_car_with_root(car_bytes)
            .await
            .map_err(|e| BackfillerError::Car(e.to_string()))?;

        // Verify repo
        let repo = self.verify_repo(car.blocks.clone(), car.root, &msg.data.did).await?;

        // Extract and write records
        self.process_repo(repo, &msg.data.did).await?;

        // ACK and delete message
        self.ack_message(&msg.id, true).await?;

        info!("Successfully processed repo for DID: {}", msg.data.did);
        Ok(())
    }

    /// Fetch repo via com.atproto.sync.getRepo
    async fn fetch_repo(&self, host: &str, did: &str) -> Result<Vec<u8>, BackfillerError> {
        let url = format!("{}/xrpc/com.atproto.sync.getRepo?did={}", host, did);

        let response = self.http_client.get(&url).send().await?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(BackfillerError::Other(anyhow::anyhow!(
                "Failed to fetch repo: {}",
                text
            )));
        }

        let bytes = response.bytes().await?.to_vec();
        Ok(bytes)
    }

    /// Verify repo signature and structure
    async fn verify_repo(
        &self,
        blocks: BlockMap,
        root: lexicon_cid::Cid,
        did: &str,
    ) -> Result<ReadableRepo, BackfillerError> {
        // Create blockstore
        let blockstore = MemoryBlockstore::new(Some(blocks))
            .await
            .map_err(|e| BackfillerError::Verification(e.to_string()))?;
        let storage = Arc::new(RwLock::new(blockstore));

        // Load repo
        let repo = ReadableRepo::load(storage, root)
            .await
            .map_err(|e| BackfillerError::Verification(e.to_string()))?;

        // Verify DID matches
        if repo.did() != did {
            return Err(BackfillerError::Verification(format!(
                "DID mismatch: expected {}, got {}",
                did,
                repo.did()
            )));
        }

        // Resolve DID to get signing key
        let did_key = self.resolve_did_key(did).await?;

        // Verify commit signature
        let valid = verify_commit_sig(repo.commit.clone(), &did_key)
            .map_err(|e| BackfillerError::Verification(e.to_string()))?;

        if !valid {
            return Err(BackfillerError::Verification(
                "Invalid commit signature".to_string(),
            ));
        }

        Ok(repo)
    }

    /// Resolve DID to get signing key
    async fn resolve_did_key(&self, did: &str) -> Result<String, BackfillerError> {
        // Use rsky-identity to resolve DID
        let opts = rsky_identity::types::IdentityResolverOpts {
            timeout: None,
            plc_url: None,
            did_cache: None,
            backup_nameservers: None,
        };
        let mut resolver = rsky_identity::IdResolver::new(opts);
        let doc = resolver
            .did
            .resolve(did.to_string(), None)
            .await
            .map_err(|e| BackfillerError::Identity(e.to_string()))?
            .ok_or_else(|| BackfillerError::Identity(format!("DID not found: {}", did)))?;

        // Extract signing key from verification methods
        if let Some(verification_methods) = &doc.verification_method {
            for vm in verification_methods {
                if let Some(key) = &vm.public_key_multibase {
                    // Return in did:key format as expected by verify_commit_sig
                    return Ok(format!("did:key:{}", key));
                }
            }
        }

        Err(BackfillerError::Identity(format!(
            "No signing key found for DID: {}",
            did
        )))
    }

    /// Process repo and write records to stream
    async fn process_repo(&self, mut repo: ReadableRepo, did: &str) -> Result<(), BackfillerError> {
        let now = chrono::Utc::now().to_rfc3339();
        let commit_cid = repo.cid.to_string();
        let rev = repo.commit.rev.clone();

        // Get all records from repo
        let leaves = repo.data.list(None, None, None)
            .await
            .map_err(|e| BackfillerError::Other(e))?;

        // Get block map from storage
        let storage_guard = repo.storage.read().await;
        let blocks_result = storage_guard.get_blocks(leaves.iter().map(|e| e.value).collect()).await
            .map_err(|e| BackfillerError::Other(e.into()))?;

        // Process in chunks of 500
        for chunk in leaves.chunks(500) {
            let mut events = Vec::new();

            for entry in chunk {
                // Parse key to get collection and rkey
                let parts: Vec<&str> = entry.key.split('/').collect();
                if parts.len() != 2 {
                    warn!("Invalid data key: {}", entry.key);
                    continue;
                }
                let collection = parts[0].to_string();
                let rkey = parts[1].to_string();

                // Get and parse record
                match get_and_parse_record(&blocks_result.blocks, entry.value) {
                    Ok(parsed) => {
                        let record_json = serde_json::to_value(&parsed.record)
                            .map_err(|e| BackfillerError::Serialization(e))?;

                        events.push(StreamEvent::Create {
                            seq: SEQ_BACKFILL,
                            time: now.clone(),
                            did: did.to_string(),
                            commit: commit_cid.clone(),
                            rev: rev.clone(),
                            collection,
                            rkey,
                            cid: entry.value.to_string(),
                            record: record_json,
                        });
                    }
                    Err(e) => {
                        warn!("Failed to parse record {}: {:?}", entry.value, e);
                    }
                }
            }

            // Add repo event at end of chunk
            events.push(StreamEvent::Repo {
                seq: SEQ_BACKFILL,
                time: now.clone(),
                did: did.to_string(),
                commit: commit_cid.clone(),
                rev: rev.clone(),
            });

            // Write events to stream
            self.write_events(&events).await?;
        }

        Ok(())
    }

    /// Write events to output stream
    async fn write_events(&self, events: &[StreamEvent]) -> Result<(), BackfillerError> {
        if events.is_empty() {
            return Ok(());
        }

        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        for event in events {
            let json = serde_json::to_string(event)?;

            let _: String = redis::cmd("XADD")
                .arg(&self.config.stream_out)
                .arg("*")
                .arg("event")
                .arg(json)
                .query_async(&mut conn)
                .await?;
        }

        Ok(())
    }

    /// ACK a message
    async fn ack_message(&self, message_id: &str, delete: bool) -> Result<(), BackfillerError> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        // ACK
        let _: i32 = redis::cmd("XACK")
            .arg(&self.config.stream_in)
            .arg(&self.config.consumer_group)
            .arg(message_id)
            .query_async(&mut conn)
            .await?;

        // Delete if requested
        if delete {
            let _: i32 = redis::cmd("XDEL")
                .arg(&self.config.stream_in)
                .arg(message_id)
                .query_async(&mut conn)
                .await?;
        }

        Ok(())
    }

    /// Clone for task (cheap clone of shared state)
    fn clone_for_task(&self) -> Self {
        Self {
            config: self.config.clone(),
            redis_client: self.redis_client.clone(),
            semaphore: self.semaphore.clone(),
            http_client: self.http_client.clone(),
        }
    }
}
