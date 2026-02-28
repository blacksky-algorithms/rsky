pub mod backfill_queue;
#[cfg(test)]
mod backfill_queue_tests;
pub mod labels;
mod tests;

use crate::SHUTDOWN;
use crate::backfiller::convert_record_to_ipld;
use crate::config::{
    CURSOR_SAVE_INTERVAL, DB_POOL_SIZE, FIREHOSE_PING_INTERVAL, INLINE_CONCURRENCY,
    WORKERS_INGESTER,
};
use crate::indexer::IndexerManager;
use crate::storage::Storage;
use crate::types::{CommitData, FirehoseEvent, IndexJob, WintermuteError, WriteAction};
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use futures::SinkExt;
use futures::stream::StreamExt;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::interval;
use tokio_postgres::NoTls;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug)]
enum ConnectionResult {
    Closed,
    Error(WintermuteError),
    FutureCursor,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ParseResult {
    Event(FirehoseEvent),
    Skip,
    OutdatedCursor,
    FutureCursor,
}

pub struct IngesterManager {
    workers: usize,
    relay_hosts: Vec<String>,
    labeler_hosts: Vec<String>,
    storage: Arc<Storage>,
    database_url: String,
}

impl IngesterManager {
    pub const fn new(
        relay_hosts: Vec<String>,
        labeler_hosts: Vec<String>,
        storage: Arc<Storage>,
        database_url: String,
    ) -> Result<Self, WintermuteError> {
        Ok(Self {
            workers: WORKERS_INGESTER,
            relay_hosts,
            labeler_hosts,
            storage,
            database_url,
        })
    }

    pub fn run(self) -> Result<(), WintermuteError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(self.workers)
            .enable_all()
            .build()
            .map_err(|e| WintermuteError::Other(format!("failed to create runtime: {e}")))?;

        rt.block_on(async {
            let mut tasks = Vec::new();

            for host in &self.relay_hosts {
                let storage = Arc::clone(&self.storage);
                let host_clone = host.clone();
                let db_url = self.database_url.clone();

                let firehose_task = tokio::spawn(async move {
                    Self::run_connection(Arc::clone(&storage), host_clone.clone(), db_url).await;
                });
                tasks.push(firehose_task);

                let backfill_storage = Arc::clone(&self.storage);
                let backfill_host = host.clone();
                let backfill_db_url = self.database_url.clone();
                let backfill_task = tokio::spawn(async move {
                    if let Err(e) = backfill_queue::populate_backfill_queue(
                        backfill_storage,
                        backfill_host,
                        backfill_db_url,
                    )
                    .await
                    {
                        tracing::error!("backfill queue population failed: {e}");
                    }
                });
                tasks.push(backfill_task);
            }

            for host in &self.labeler_hosts {
                let storage = Arc::clone(&self.storage);
                let host = host.clone();
                let db_url = self.database_url.clone();

                let task = tokio::spawn(async move {
                    if let Err(e) = labels::subscribe_labels(storage, host, db_url).await {
                        tracing::error!("label subscription failed: {e}");
                    }
                });

                tasks.push(task);
            }

            for task in tasks {
                drop(task.await);
            }
        });

        Ok(())
    }

    async fn run_connection(storage: Arc<Storage>, hostname: String, database_url: String) {
        // Create postgres pool for cursor storage AND direct indexing
        let pool_size = *DB_POOL_SIZE;
        tracing::info!("firehose DB pool size: {pool_size}");
        let mut pg_config = Config::new();
        pg_config.url = Some(database_url);
        pg_config.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });
        pg_config.pool = Some(deadpool_postgres::PoolConfig::new(pool_size));

        let pool = match pg_config.create_pool(Some(Runtime::Tokio1), NoTls) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                tracing::error!("failed to create firehose pool: {e}");
                return;
            }
        };

        // Semaphore to limit concurrent indexing tasks (configurable via INLINE_CONCURRENCY)
        let concurrency = *INLINE_CONCURRENCY;
        tracing::info!("firehose inline concurrency: {concurrency}");
        let semaphore = Arc::new(Semaphore::new(concurrency));

        // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 32s, 60s max
        let max_backoff_secs = 60u64;
        let mut backoff_secs = 1u64;

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!("shutdown requested for {hostname}");
                break;
            }

            match Self::connect_and_stream(&storage, &hostname, &pool, &semaphore).await {
                ConnectionResult::Closed => {
                    if SHUTDOWN.load(Ordering::Relaxed) {
                        break;
                    }
                    tracing::warn!(
                        "connection closed for {hostname}, reconnecting in {backoff_secs}s"
                    );
                    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(max_backoff_secs);
                }
                ConnectionResult::Error(e) => {
                    if SHUTDOWN.load(Ordering::Relaxed) {
                        break;
                    }
                    tracing::error!(
                        "connection error for {hostname}: {e}, retrying in {backoff_secs}s"
                    );
                    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(max_backoff_secs);
                }
                ConnectionResult::FutureCursor => {
                    // Cursor is somehow ahead of relay (wrong cursor for wrong relay, etc)
                    // Delete cursor and reconnect from 0 to let relay give us everything
                    let cursor_key = format!("firehose:{hostname}");
                    tracing::warn!(
                        "cursor in future for {hostname}, deleting cursor to restart from 0"
                    );
                    if let Err(e) = storage.delete_cursor(&cursor_key) {
                        tracing::error!("failed to delete cursor from fjall: {e}");
                    }
                    if let Err(e) = delete_cursor_from_postgres(&pool, &cursor_key).await {
                        tracing::error!("failed to delete cursor from postgres: {e}");
                    }
                    // Reset backoff since we're intentionally reconnecting with new state
                    backoff_secs = 1;
                }
            }
        }
    }

    async fn connect_and_stream(
        _storage: &Storage,
        hostname: &str,
        pool: &Arc<Pool>,
        semaphore: &Arc<Semaphore>,
    ) -> ConnectionResult {
        use crate::metrics;

        let cursor_key = format!("firehose:{hostname}");

        // Use AtomicI64 for cheap, lock-free cursor updates (like indigo/tap's lastSeq)
        let last_seq = Arc::new(AtomicI64::new(0));

        // Get cursor from postgres (survives Fjall corruption)
        let cursor = match get_cursor_from_postgres(pool, &cursor_key).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("failed to get cursor from postgres: {e}");
                return ConnectionResult::Error(e);
            }
        };

        let clean_hostname = hostname
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');

        let url = match url::Url::parse(&format!(
            "wss://{clean_hostname}/xrpc/com.atproto.sync.subscribeRepos"
        )) {
            Ok(mut u) => {
                // Only add cursor if we have a saved position
                // No cursor = start from current stream position (live)
                // cursor=N = resume from seq N (may be in rollback window)
                if cursor > 0 {
                    u.query_pairs_mut()
                        .append_pair("cursor", &cursor.to_string());
                }
                u
            }
            Err(e) => {
                return ConnectionResult::Error(WintermuteError::Other(format!(
                    "invalid url: {e}"
                )));
            }
        };

        if cursor > 0 {
            tracing::info!("connecting to {url} resuming from cursor {cursor}");
        } else {
            tracing::info!("connecting to {url} starting from live stream");
        }

        let (ws_stream, _) = match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok(s) => s,
            Err(e) => return ConnectionResult::Error(e.into()),
        };

        metrics::INGESTER_WEBSOCKET_CONNECTIONS
            .with_label_values(&["firehose"])
            .inc();
        let (mut write, mut read) = ws_stream.split();

        let ping_task = tokio::spawn(async move {
            let mut ping_interval = interval(FIREHOSE_PING_INTERVAL);
            loop {
                ping_interval.tick().await;
                if write.send(Message::Ping(vec![])).await.is_err() {
                    break;
                }
            }
        });

        // Spawn interval-based cursor saver (like indigo/tap's RunCursorSaver)
        // This prevents Fjall poisoning from high-frequency per-event writes
        let cursor_saver_seq = Arc::clone(&last_seq);
        let cursor_saver_pool = Arc::clone(pool);
        let cursor_saver_key = cursor_key.clone();
        let cursor_saver_task = tokio::spawn(async move {
            let mut cursor_interval = interval(*CURSOR_SAVE_INTERVAL);
            let mut last_saved_seq = 0i64;
            loop {
                cursor_interval.tick().await;
                if SHUTDOWN.load(Ordering::Relaxed) {
                    break;
                }
                let current_seq = cursor_saver_seq.load(Ordering::Relaxed);
                if current_seq > 0 && current_seq != last_saved_seq {
                    if let Err(e) =
                        set_cursor_in_postgres(&cursor_saver_pool, &cursor_saver_key, current_seq)
                            .await
                    {
                        tracing::error!("cursor saver failed to save to postgres: {e}");
                        metrics::INGESTER_ERRORS_TOTAL
                            .with_label_values(&["firehose_cursor"])
                            .inc();
                    } else {
                        last_saved_seq = current_seq;
                        tracing::debug!("cursor saved: {current_seq}");
                    }
                }
            }
        });

        loop {
            // Check shutdown with timeout to avoid blocking forever on websocket read
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!("shutdown requested, closing firehose connection");
                // Save final cursor before shutdown
                let final_seq = last_seq.load(Ordering::Relaxed);
                if final_seq > 0 {
                    if let Err(e) = set_cursor_in_postgres(pool, &cursor_key, final_seq).await {
                        tracing::error!("failed to save cursor on shutdown: {e}");
                    }
                }
                metrics::INGESTER_WEBSOCKET_CONNECTIONS
                    .with_label_values(&["firehose"])
                    .dec();
                ping_task.abort();
                cursor_saver_task.abort();
                return ConnectionResult::Closed;
            }

            let msg = tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => {
                            // Save cursor before returning on error
                            let final_seq = last_seq.load(Ordering::Relaxed);
                            if final_seq > 0 {
                                drop(set_cursor_in_postgres(pool, &cursor_key, final_seq).await);
                            }
                            metrics::INGESTER_WEBSOCKET_CONNECTIONS
                                .with_label_values(&["firehose"])
                                .dec();
                            ping_task.abort();
                            cursor_saver_task.abort();
                            return ConnectionResult::Error(e.into());
                        }
                        None => break,
                    }
                }
                () = tokio::time::sleep(Duration::from_millis(100)) => continue,
            };

            if let Message::Binary(data) = msg {
                let event = match Self::parse_message(&data) {
                    Ok(ParseResult::Event(e)) => e,
                    Ok(ParseResult::Skip) => continue,
                    Ok(ParseResult::OutdatedCursor) => {
                        // Per AT Protocol spec: after OutdatedCursor info, relay continues
                        // from its oldest available position. We just continue processing.
                        tracing::warn!(
                            "cursor too old - relay will resume from oldest available position"
                        );
                        continue;
                    }
                    Ok(ParseResult::FutureCursor) => {
                        // Cursor is ahead of relay - delete cursor and reconnect from 0
                        tracing::warn!("cursor in future - relay closed connection");
                        metrics::INGESTER_WEBSOCKET_CONNECTIONS
                            .with_label_values(&["firehose"])
                            .dec();
                        ping_task.abort();
                        cursor_saver_task.abort();
                        return ConnectionResult::FutureCursor;
                    }
                    Err(e) => {
                        tracing::warn!("failed to parse message: {e}");
                        continue;
                    }
                };

                metrics::INGESTER_FIREHOSE_EVENTS_TOTAL
                    .with_label_values(&["firehose_live"])
                    .inc();

                // Handle identity events separately (handle changes, key rotations)
                if event.kind == "identity" {
                    let pool_clone = Arc::clone(pool);
                    let event_did = event.did.clone();
                    let event_time = event.time.clone();
                    let event_handle = event.identity.as_ref().and_then(|i| i.handle.clone());
                    tokio::spawn(async move {
                        if let Err(e) = Self::process_identity_event(
                            &pool_clone,
                            &event_did,
                            &event_time,
                            event_handle.as_deref(),
                        )
                        .await
                        {
                            tracing::error!(
                                "identity event processing failed for {}: {e}",
                                event_did
                            );
                            metrics::INGESTER_ERRORS_TOTAL
                                .with_label_values(&["identity_failed"])
                                .inc();
                        }
                    });
                    last_seq.store(event.seq, Ordering::Relaxed);
                    continue;
                }

                // Handle account events (takedown, suspension, deletion, reactivation)
                if event.kind == "account" {
                    if let Some(ref account) = event.account {
                        let pool_clone = Arc::clone(pool);
                        let event_did = event.did.clone();
                        let active = account.active;
                        let status = account.status.clone();
                        tokio::spawn(async move {
                            if let Err(e) = Self::process_account_event(
                                &pool_clone,
                                &event_did,
                                active,
                                status.as_deref(),
                            )
                            .await
                            {
                                tracing::error!(
                                    "account event processing failed for {}: {e}",
                                    event_did
                                );
                                metrics::INGESTER_ERRORS_TOTAL
                                    .with_label_values(&["account_failed"])
                                    .inc();
                            }
                        });
                    }
                    last_seq.store(event.seq, Ordering::Relaxed);
                    continue;
                }

                // Handle sync events (repo recovery - refresh handle like identity events)
                if event.kind == "sync" {
                    let pool_clone = Arc::clone(pool);
                    let event_did = event.did.clone();
                    let event_time = event.time.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            Self::process_identity_event(&pool_clone, &event_did, &event_time, None)
                                .await
                        {
                            tracing::error!("sync event processing failed for {}: {e}", event_did);
                            metrics::INGESTER_ERRORS_TOTAL
                                .with_label_values(&["sync_failed"])
                                .inc();
                        }
                    });
                    last_seq.store(event.seq, Ordering::Relaxed);
                    continue;
                }

                // Process inline: parse event and spawn indexing tasks directly (skip Fjall queue)
                match Self::parse_event_to_jobs(&event).await {
                    Ok(jobs) => {
                        for job in jobs {
                            // Acquire semaphore permit (like rsky-firehose)
                            let Ok(permit) = semaphore.clone().acquire_owned().await else {
                                tracing::error!("semaphore closed during firehose processing");
                                break;
                            };
                            let pool_clone = Arc::clone(pool);
                            tokio::spawn(async move {
                                if let Err(e) = IndexerManager::process_job(&pool_clone, &job).await
                                {
                                    tracing::error!("inline indexing failed: {e}");
                                    metrics::INDEXER_RECORDS_FAILED_TOTAL.inc();
                                }
                                drop(permit);
                            });
                        }
                    }
                    Err(e) => {
                        tracing::warn!("failed to parse event seq={} to jobs: {e}", event.seq);
                        metrics::INGESTER_ERRORS_TOTAL
                            .with_label_values(&["parse_failed"])
                            .inc();
                    }
                }

                // Atomically update last_seq (cheap, lock-free operation)
                // The cursor_saver_task will persist this to postgres on interval
                last_seq.store(event.seq, Ordering::Relaxed);
            }
        }

        // Save final cursor before disconnecting
        let final_seq = last_seq.load(Ordering::Relaxed);
        if final_seq > 0 {
            if let Err(e) = set_cursor_in_postgres(pool, &cursor_key, final_seq).await {
                tracing::error!("failed to save final cursor: {e}");
            }
        }

        metrics::INGESTER_WEBSOCKET_CONNECTIONS
            .with_label_values(&["firehose"])
            .dec();
        ping_task.abort();
        cursor_saver_task.abort();
        ConnectionResult::Closed
    }

    pub fn parse_message(data: &[u8]) -> Result<ParseResult, WintermuteError> {
        // AT Protocol sends two concatenated CBOR messages:
        // 1. Header (parsed with ciborium): {t: "#commit", op: 1}
        // 2. Body (parsed with serde_ipld_dagcbor): {seq: N, repo: "...", ...}

        #[derive(serde::Deserialize)]
        struct Header {
            #[serde(rename = "t")]
            type_: String,
            #[serde(rename = "op")]
            _operation: u8,
        }

        #[derive(serde::Deserialize)]
        struct InfoBody {
            name: String,
            message: Option<String>,
        }

        let mut cursor = std::io::Cursor::new(data);

        // Parse header with ciborium
        let header: Header = ciborium::from_reader(&mut cursor)
            .map_err(|e| WintermuteError::Serialization(format!("failed to parse header: {e}")))?;

        // Handle #info messages (cursor too old, etc)
        if header.type_ == "#info" {
            if let Ok(info) = serde_ipld_dagcbor::from_reader::<InfoBody, _>(&mut cursor) {
                tracing::info!(
                    "received #info: name={}, message={:?}",
                    info.name,
                    info.message
                );
                // OutdatedCursor means the cursor is too old - relay continues from oldest
                if info.name == "OutdatedCursor" {
                    return Ok(ParseResult::OutdatedCursor);
                }
            }
            return Ok(ParseResult::Skip);
        }

        // Handle #error messages (future cursor, invalid request, etc)
        if header.type_ == "#error" {
            if let Ok(error) = serde_ipld_dagcbor::from_reader::<InfoBody, _>(&mut cursor) {
                tracing::error!(
                    "received #error: name={}, message={:?}",
                    error.name,
                    error.message
                );
                // FutureCursor means cursor is ahead of relay - need to reset
                if error.name == "FutureCursor" {
                    return Ok(ParseResult::FutureCursor);
                }
            }
            return Ok(ParseResult::Skip);
        }

        // Handle #identity events (handle changes, key rotations, etc)
        if header.type_ == "#identity" {
            let body: rsky_lexicon::com::atproto::sync::SubscribeReposIdentity =
                serde_ipld_dagcbor::from_reader(&mut cursor).map_err(|e| {
                    WintermuteError::Serialization(format!("failed to parse identity body: {e}"))
                })?;

            let event = FirehoseEvent {
                seq: body.seq,
                did: body.did,
                time: body.time.to_rfc3339(),
                kind: "identity".to_owned(),
                commit: None,
                identity: Some(crate::types::IdentityData {
                    handle: body.handle,
                }),
                account: None,
            };

            return Ok(ParseResult::Event(event));
        }

        // Handle #account events (takedown, suspension, deletion, etc.)
        if header.type_ == "#account" {
            let body: rsky_lexicon::com::atproto::sync::SubscribeReposAccount =
                serde_ipld_dagcbor::from_reader(&mut cursor).map_err(|e| {
                    WintermuteError::Serialization(format!("failed to parse account body: {e}"))
                })?;

            let event = FirehoseEvent {
                seq: body.seq,
                did: body.did,
                time: body.time.to_rfc3339(),
                kind: "account".to_owned(),
                commit: None,
                identity: None,
                account: Some(crate::types::AccountData {
                    active: body.active,
                    status: body.status.map(|s| s.to_string().to_lowercase()),
                }),
            };

            return Ok(ParseResult::Event(event));
        }

        // Handle #sync events (repo state recovery/updates)
        if header.type_ == "#sync" {
            let body: rsky_lexicon::com::atproto::sync::SubscribeReposSync =
                serde_ipld_dagcbor::from_reader(&mut cursor).map_err(|e| {
                    WintermuteError::Serialization(format!("failed to parse sync body: {e}"))
                })?;

            // Treat sync like identity - refresh the handle
            let event = FirehoseEvent {
                seq: body.seq,
                did: body.did,
                time: body.time.to_rfc3339(),
                kind: "sync".to_owned(),
                commit: None,
                identity: None,
                account: None,
            };

            return Ok(ParseResult::Event(event));
        }

        // Only process #commit messages beyond this point
        if header.type_ != "#commit" {
            return Ok(ParseResult::Skip);
        }

        // Parse body with serde_ipld_dagcbor using the official SubscribeReposCommit struct
        let body: rsky_lexicon::com::atproto::sync::SubscribeReposCommit =
            serde_ipld_dagcbor::from_reader(&mut cursor).map_err(|e| {
                WintermuteError::Serialization(format!("failed to parse body: {e}"))
            })?;

        // Convert operations to our format
        let ops = body
            .ops
            .into_iter()
            .map(|op| crate::types::RepoOp {
                action: op.action,
                path: op.path,
                cid: op.cid.map(|c| c.to_string()),
            })
            .collect();

        let event = FirehoseEvent {
            seq: body.seq,
            did: body.repo,
            time: body.time.to_rfc3339(),
            kind: "commit".to_owned(),
            commit: Some(CommitData {
                rev: body.rev,
                ops,
                blocks: body.blocks,
            }),
            identity: None,
            account: None,
        };

        Ok(ParseResult::Event(event))
    }

    /// Parse a `FirehoseEvent` into `IndexJob`s for inline processing (skipping Fjall queue)
    pub async fn parse_event_to_jobs(
        event: &FirehoseEvent,
    ) -> Result<Vec<IndexJob>, WintermuteError> {
        use rsky_repo::parse::get_and_parse_record;

        let mut jobs = Vec::new();

        // Only process commit events with operations
        let Some(ref commit) = event.commit else {
            return Ok(jobs);
        };

        if commit.ops.is_empty() {
            return Ok(jobs);
        }

        // Parse CAR blocks into a BlockMap
        let block_map = if commit.blocks.is_empty() {
            None
        } else {
            let cursor = std::io::Cursor::new(&commit.blocks);
            match iroh_car::CarReader::new(cursor).await {
                Ok(mut car_reader) => {
                    let mut block_map = rsky_repo::block_map::BlockMap::new();
                    while let Ok(Some((cid, data))) = car_reader.next_block().await {
                        block_map.set(cid, data);
                    }
                    Some(block_map)
                }
                Err(e) => {
                    tracing::warn!("failed to parse CAR blocks for seq={}: {e}", event.seq);
                    None
                }
            }
        };

        let indexed_at = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        // Convert each operation to an IndexJob
        for op in &commit.ops {
            let action = match op.action.as_str() {
                "create" => WriteAction::Create,
                "update" => WriteAction::Update,
                "delete" => WriteAction::Delete,
                _ => {
                    tracing::warn!("unknown action type: {}", op.action);
                    continue;
                }
            };

            // Build AT-URI from repo DID + record path
            let uri = format!("at://{}/{}", event.did, op.path);

            // Use CID from operation, or empty string for deletes
            let cid_str = op.cid.clone().unwrap_or_default();

            // Extract record from CAR blocks if we have a CID and block_map
            let record = if let (Some(cid_value), Some(blocks)) = (&op.cid, &block_map) {
                match lexicon_cid::Cid::try_from(cid_value.as_str()) {
                    Ok(cid) => match get_and_parse_record(blocks, cid) {
                        Ok(parsed) => match serde_json::to_value(&parsed.record) {
                            Ok(record_json) => Some(convert_record_to_ipld(&record_json)),
                            Err(e) => {
                                tracing::warn!("failed to serialize record for {uri}: {e}");
                                None
                            }
                        },
                        Err(e) => {
                            tracing::warn!("failed to parse record for {uri}: {e}");
                            None
                        }
                    },
                    Err(e) => {
                        tracing::warn!("invalid CID {cid_value} for {uri}: {e}");
                        None
                    }
                }
            } else {
                None
            };

            jobs.push(IndexJob {
                uri,
                cid: cid_str,
                action,
                record,
                indexed_at: indexed_at.clone(),
                rev: commit.rev.clone(),
            });
        }

        Ok(jobs)
    }

    pub async fn enqueue_event_for_indexing(
        storage: &Storage,
        event: &FirehoseEvent,
    ) -> Result<(), WintermuteError> {
        use rsky_repo::parse::get_and_parse_record;

        // Only process commit events with operations
        let Some(ref commit) = event.commit else {
            return Ok(());
        };

        if commit.ops.is_empty() {
            return Ok(());
        }

        // Parse CAR blocks into a BlockMap
        let block_map = if commit.blocks.is_empty() {
            None
        } else {
            let cursor = std::io::Cursor::new(&commit.blocks);
            match iroh_car::CarReader::new(cursor).await {
                Ok(mut car_reader) => {
                    let mut block_map = rsky_repo::block_map::BlockMap::new();
                    while let Ok(Some((cid, data))) = car_reader.next_block().await {
                        block_map.set(cid, data);
                    }
                    Some(block_map)
                }
                Err(e) => {
                    tracing::warn!("failed to parse CAR blocks for seq={}: {e}", event.seq);
                    None
                }
            }
        };

        let indexed_at = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        // Convert each operation to an IndexJob
        for op in &commit.ops {
            let action = match op.action.as_str() {
                "create" => crate::types::WriteAction::Create,
                "update" => crate::types::WriteAction::Update,
                "delete" => crate::types::WriteAction::Delete,
                _ => {
                    tracing::warn!("unknown action type: {}", op.action);
                    continue;
                }
            };

            // Build AT-URI from repo DID + record path
            let uri = format!("at://{}/{}", event.did, op.path);

            // Use CID from operation, or empty string for deletes
            let cid_str = op.cid.clone().unwrap_or_default();

            // Extract record from CAR blocks if we have a CID and block_map
            let record = if let (Some(cid_value), Some(blocks)) = (&op.cid, &block_map) {
                match lexicon_cid::Cid::try_from(cid_value.as_str()) {
                    Ok(cid) => match get_and_parse_record(blocks, cid) {
                        Ok(parsed) => match serde_json::to_value(&parsed.record) {
                            Ok(record_json) => Some(convert_record_to_ipld(&record_json)),
                            Err(e) => {
                                tracing::warn!("failed to serialize record for {uri}: {e}");
                                None
                            }
                        },
                        Err(e) => {
                            tracing::warn!("failed to parse record for {uri}: {e}");
                            None
                        }
                    },
                    Err(e) => {
                        tracing::warn!("invalid CID {cid_value} for {uri}: {e}");
                        None
                    }
                }
            } else {
                None
            };

            let job = crate::types::IndexJob {
                uri,
                cid: cid_str,
                action,
                record,
                indexed_at: indexed_at.clone(),
                rev: commit.rev.clone(),
            };

            storage.enqueue_firehose_live(&job)?;
        }

        Ok(())
    }

    /// Process an identity event by resolving the DID and updating the actor table
    async fn process_identity_event(
        pool: &Pool,
        did: &str,
        timestamp: &str,
        handle_hint: Option<&str>,
    ) -> Result<(), WintermuteError> {
        use rsky_identity::IdResolver;
        use rsky_identity::types::IdentityResolverOpts;

        tracing::debug!("processing identity event for {}", did);

        // If the event includes the handle, we can use it directly
        // Otherwise, resolve the DID to get the current handle from the DID document
        let handle = if let Some(h) = handle_hint {
            Some(h.to_lowercase())
        } else {
            // Resolve DID to get current handle from DID document
            let mut resolver = IdResolver::new(IdentityResolverOpts {
                timeout: Some(std::time::Duration::from_secs(5)),
                plc_url: None,
                did_cache: None,
                backup_nameservers: None,
            });

            match resolver.did.resolve(did.to_owned(), None).await {
                Ok(Some(doc)) => {
                    // Extract handle from alsoKnownAs (at:// URIs)
                    let handle = doc.also_known_as.as_ref().and_then(|akas| {
                        akas.iter()
                            .find(|aka| aka.starts_with("at://"))
                            .map(|aka| aka.strip_prefix("at://").unwrap_or(aka).to_lowercase())
                    });

                    if let Some(ref h) = handle {
                        // Verify handle resolves back to this DID
                        match resolver.handle.resolve(h).await {
                            Ok(Some(resolved_did)) if resolved_did == did => {
                                tracing::info!("identity event: verified handle {} for {}", h, did);
                            }
                            _ => {
                                tracing::debug!(
                                    "handle {} does not resolve back to {} - setting handle to null",
                                    h,
                                    did
                                );
                                return Ok(()); // Don't update if handle doesn't verify
                            }
                        }
                    }

                    handle
                }
                Ok(None) => {
                    tracing::warn!("DID {} not found", did);
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("failed to resolve DID {}: {}", did, e);
                    return Ok(()); // Don't fail on resolution errors, just skip
                }
            }
        };

        // Update actor table
        let client = pool.get().await?;
        let result = client
            .execute(
                "UPDATE actor SET handle = $1, \"indexedAt\" = $2 WHERE did = $3",
                &[&handle, &timestamp, &did],
            )
            .await?;

        if result > 0 {
            tracing::info!(
                "updated handle for {} to {:?}",
                did,
                handle.as_deref().unwrap_or("null")
            );
        } else {
            tracing::debug!("no actor found to update for {}", did);
        }

        Ok(())
    }

    /// Process an account event by updating the actor's upstream status
    async fn process_account_event(
        pool: &Pool,
        did: &str,
        active: bool,
        status: Option<&str>,
    ) -> Result<(), WintermuteError> {
        tracing::debug!(
            "processing account event for {}: active={}, status={:?}",
            did,
            active,
            status
        );

        // Determine upstream_status based on active flag and status
        let upstream_status: Option<&str> = if active {
            // Active accounts have no upstream status
            None
        } else {
            // Inactive accounts: check for recognized statuses
            match status {
                Some(s) if ["deactivated", "suspended", "takendown", "deleted"].contains(&s) => {
                    Some(s)
                }
                Some(s) => {
                    tracing::warn!("unrecognized account status '{}' for {}", s, did);
                    Some(s) // Still store it, just log a warning
                }
                None => {
                    tracing::warn!("inactive account {} has no status", did);
                    None
                }
            }
        };

        // Update actor table
        let client = pool.get().await?;
        let result = client
            .execute(
                "UPDATE actor SET \"upstreamStatus\" = $1 WHERE did = $2",
                &[&upstream_status, &did],
            )
            .await?;

        if result > 0 {
            tracing::info!(
                "updated upstream_status for {} to {:?}",
                did,
                upstream_status.unwrap_or("null")
            );
        } else {
            tracing::debug!("no actor found to update status for {}", did);
        }

        Ok(())
    }
}

async fn get_cursor_from_postgres(pool: &Pool, service: &str) -> Result<i64, WintermuteError> {
    let client = pool.get().await?;
    let row = client
        .query_opt(
            "SELECT cursor FROM sub_state WHERE service = $1",
            &[&service],
        )
        .await?;

    Ok(row.map_or(0, |r| r.get::<_, i64>("cursor")))
}

async fn set_cursor_in_postgres(
    pool: &Pool,
    service: &str,
    cursor: i64,
) -> Result<(), WintermuteError> {
    let client = pool.get().await?;
    client
        .execute(
            "INSERT INTO sub_state (service, cursor)
             VALUES ($1, $2)
             ON CONFLICT (service) DO UPDATE SET cursor = EXCLUDED.cursor",
            &[&service, &cursor],
        )
        .await?;
    Ok(())
}

async fn delete_cursor_from_postgres(pool: &Pool, service: &str) -> Result<(), WintermuteError> {
    let client = pool.get().await?;
    client
        .execute("DELETE FROM sub_state WHERE service = $1", &[&service])
        .await?;
    Ok(())
}
