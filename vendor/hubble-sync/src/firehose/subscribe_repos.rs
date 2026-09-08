//! a `com.atproto.sync.subscribeRepos` websocket -> event dispatcher

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jacquard_api::com_atproto::sync::subscribe_repos::SubscribeReposMessage;
use jacquard_common::stream::StreamErrorKind;
use jacquard_common::types::datetime::Datetime;
use jacquard_common::websocket::tungstenite_client::TungsteniteClient;
use jacquard_common::xrpc::{SubscriptionExt, SubscriptionStream};
use metrics::{counter, gauge};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

use super::tolerant::{TolerantSubscribeRepos, TolerantSubscribeReposStream};
use super::{
    FirehoseAck, FirehoseCommit, FirehoseEvent, FirehosePayload, FirehoseSync,
    FirehoseValidationError, Seq,
};
use crate::identity::Resolve;
use crate::metrics::{
    FIREHOSE_CONNECTS_TOTAL, FIREHOSE_CURSOR_SEQ, FIREHOSE_DECODE_ERRORS_TOTAL,
    FIREHOSE_INVALID_SEQ_TOTAL, FIREHOSE_MESSAGES_TOTAL, FIREHOSE_OUTSTANDING,
    FIREHOSE_RECONNECTS_TOTAL, FIREHOSE_REPO_BACKPRESSURE_EXHAUSTED_TOTAL,
    FIREHOSE_REPO_BACKPRESSURE_RETRIES_TOTAL, FIREHOSE_STALL_EVICTIONS_TOTAL,
    FIREHOSE_UPSTREAM_LAG_SECONDS, FIREHOSE_VALIDATE_FAIL_TOTAL,
};
use crate::repo_actor::{RepoMessage, RepoSendError};
use crate::storage::engine::{StorageBatch, StorageError};
use crate::storage::firehose_cursor::CursorState;
use crate::storage::{LoadError, StorageEngine};
use crate::{CancelExt, Did, Host, PrefixedEngine, RepoRegistry, SyncConsumer};

#[derive(Debug, Clone)]
pub struct FirehoseConfig {
    /// firehose backpressure
    ///
    /// outstanding events have been dispatched but not yet acked.
    ///
    /// default: 2048
    pub max_outstanding: usize,
    /// close and reconnect if no new events have arrived after
    ///
    /// default: 15s
    pub idle_timeout: Duration,
    /// save the cursor this often
    ///
    /// slightly different from the same Tap setting: Tap saves the last-
    /// arriving seq, so when it exits, any still-processing events are lost and
    /// don't replay on restart. those repos are left silently out-of-sync, and
    /// (usually) fail to prove their next-arriving event, triggering resync.
    ///
    /// hubble-sync acks each event back to the firehose consumer, so we can
    /// save the oldest-unacknowledged-event's seq instead, so stop + restart
    /// doesn't miss anything.
    ///
    /// default: 1s
    pub cursor_persist_interval: Duration,
    /// how old an unacknowledged event can get while still holding back the
    /// saved cursor seq.
    ///
    /// default: 16s
    pub cursor_unack_hold_limit: Duration,
    /// repo-level backpressure: sleep this long before retrying
    ///
    /// response to getting `RepoSendError::Backpressure` on dispatch.
    ///
    /// in the future, we might consider leaving repos behind if they
    /// individually can't keep up, but that ideally shouldn't happen.
    ///
    /// default: 16ms
    pub repo_backpressure_retry_sleep: Duration,
    /// limit how many per-repo retries we'll attempt before giving up
    ///
    /// default: 128
    pub repo_backpressure_max_retries: u32,
    /// the longest to wait between websocket reconnect attempts
    ///
    /// default: 48s
    pub reconnect_max_backoff: Duration,
}

impl Default for FirehoseConfig {
    fn default() -> Self {
        Self {
            max_outstanding: 2048,
            idle_timeout: Duration::from_secs(15),
            cursor_persist_interval: Duration::from_secs(1),
            cursor_unack_hold_limit: Duration::from_secs(16),
            repo_backpressure_retry_sleep: Duration::from_millis(16),
            repo_backpressure_max_retries: 128,
            reconnect_max_backoff: Duration::from_secs(48),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FirehoseError<E: StorageError> {
    #[error("storage load: {0}")]
    StorageLoad(#[source] LoadError<E>),
    #[error("storage: {0}")]
    Storage(#[source] E),
    #[error("invalid sequence number: {0}")]
    InvalidSeq(i64),
    #[error("firehose setup error: {0}")]
    StreamSetupError(String),
    #[error("max repo backpressure retry attempts reached for repo: {0:?}")]
    RepoBackpressureExhausted(Did),
    #[error("repo actor registry full and couldn't evict any")]
    RepoEvictionStuck,
    #[error("cancelled")]
    Cancelled,
}

struct SubProgress {
    outstanding: BTreeMap<Seq, Instant>,
    last_seq: Seq,
    cursor_tick: Instant,
    backoff: Duration,
}

#[derive(Debug)]
pub struct FirehoseSubscriber<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> {
    host: Arc<Host>,
    registry: Arc<RepoRegistry<S, A, R>>,
    storage: PrefixedEngine<S>,
    config: FirehoseConfig,
}

impl<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> FirehoseSubscriber<S, A, R> {
    pub fn new(
        host: Arc<Host>,
        registry: Arc<RepoRegistry<S, A, R>>,
        storage: PrefixedEngine<S>,
        config: FirehoseConfig,
    ) -> Self {
        Self {
            host,
            registry,
            storage,
            config,
        }
    }

    pub fn host(&self) -> &Arc<Host> {
        &self.host
    }

    pub async fn run(&mut self, cancel: CancellationToken) -> Result<(), FirehoseError<S::Error>> {
        let (ack_tx, mut ack_rx) = mpsc::unbounded_channel::<Seq>();
        let mut progress = SubProgress {
            outstanding: BTreeMap::new(),
            last_seq: Seq(0),
            cursor_tick: Instant::now(),
            backoff: Duration::from_secs(1),
        };

        'reconnect: loop {
            if cancel.is_cancelled() {
                return Ok(());
            }
            let cursor = self.load_cursor().await?;
            if let Some(cs) = cursor.as_ref() {
                progress.last_seq = progress.last_seq.max(cs.seq);
            }
            info!(host = %self.host.name(), cursor = ?cursor, "connecting to firehose");

            let stream = match cancel.run(self.connect(cursor.map(|c| c.seq))).await {
                None => return Ok(()),
                Some(Ok(s)) => {
                    progress.backoff = Duration::from_secs(1);
                    s
                }
                Some(Err(e)) => {
                    warn!(error = %e, host = %self.host.name(),
                        backoff_ms = progress.backoff.as_millis(),
                        "firehose connect failed, will retry");
                    if !cancel.sleep(progress.backoff).await {
                        return Ok(());
                    }
                    progress.backoff =
                        (progress.backoff * 2).min(self.config.reconnect_max_backoff);
                    continue 'reconnect;
                }
            };

            info!(host = %self.host.name(), "firehose connected");

            match self
                .consume(stream, &mut progress, &ack_tx, &mut ack_rx, &cancel)
                .await?
            {
                ConnExit::Cancelled => return Ok(()),
                ConnExit::Reconnect => {
                    if !cancel.sleep(progress.backoff).await {
                        return Ok(());
                    }
                    progress.backoff =
                        (progress.backoff * 2).min(self.config.reconnect_max_backoff);
                    continue 'reconnect;
                }
                ConnExit::ReconnectNow => continue 'reconnect,
            }
        }
    }

    async fn consume(
        &self,
        stream: SubscriptionStream<TolerantSubscribeReposStream>,
        progress: &mut SubProgress,
        ack_tx: &mpsc::UnboundedSender<Seq>,
        ack_rx: &mut mpsc::UnboundedReceiver<Seq>,
        cancel: &CancellationToken,
    ) -> Result<ConnExit, FirehoseError<S::Error>> {
        let (_sink, mut messages) = stream.into_stream();
        let mut last_msg = Instant::now();
        loop {
            let can_accept = progress.outstanding.len() < self.config.max_outstanding;
            let idle_deadline = self.config.idle_timeout.saturating_sub(last_msg.elapsed());
            tokio::select! {
                biased;
                // first priority: don't keep working if we're cancelled
                slept = cancel.sleep(idle_deadline) => if slept {
                    warn!(host = %self.host.name(),
                        timeout_secs = self.config.idle_timeout.as_secs(),
                        "firehose idle timeout, reconnecting");
                    counter!(FIREHOSE_RECONNECTS_TOTAL, "reason" => "idle_timeout").increment(1);
                    return Ok(ConnExit::ReconnectNow);
                } else {
                    return Ok(ConnExit::Cancelled);
                },
                // second: clear outstanding acks before creating new ones
                Some(seq) = ack_rx.recv() => {
                    progress.outstanding.remove(&seq);
                    gauge!(FIREHOSE_OUTSTANDING).set(progress.outstanding.len() as f64);
                },
                // finally (if possible): process new messages
                msg = messages.next(), if can_accept => {
                    let Some(m) = msg else {
                        info!(host = %self.host.name(), "firehose stream ended, reconnecting");
                        counter!(FIREHOSE_RECONNECTS_TOTAL, "reason" => "stream_end").increment(1);
                        return Ok(ConnExit::ReconnectNow);
                    };
                    match m {
                        Ok(m) => {
                            // successful message receipt: update normal-operation states
                            last_msg = Instant::now();
                            progress.backoff = Duration::from_secs(1);
                            let now = SystemTime::now();
                            if let Some(seq) = self.dispatch(m, now, ack_tx, cancel).await? {
                                progress.outstanding.insert(seq, Instant::now());
                                gauge!(FIREHOSE_OUTSTANDING).set(progress.outstanding.len() as f64);
                                progress.last_seq = seq;
                            }
                        },
                        Err(e) => match e.kind() {
                            StreamErrorKind::Decode | StreamErrorKind::WrongMessageFormat => {
                                counter!(FIREHOSE_DECODE_ERRORS_TOTAL).increment(1);
                                warn!(error = %e, host = %self.host.name(), "firehose decode error, skipping");
                            }
                            StreamErrorKind::Closed => {
                                info!(host = %self.host.name(), "firehose stream closed, reconnecting");
                                counter!(FIREHOSE_RECONNECTS_TOTAL, "reason" => "closed").increment(1);
                                return Ok(ConnExit::ReconnectNow);
                            }
                            _ => {
                                warn!(error = %e, host = %self.host.name(),
                                    backoff_ms = progress.backoff.as_millis(),
                                    "firehose stream transport error, reconnecting");
                                counter!(FIREHOSE_RECONNECTS_TOTAL, "reason" => "transport_error").increment(1);
                                return Ok(ConnExit::Reconnect);
                            }
                        }
                    }
                    if progress.cursor_tick.elapsed() >= self.config.cursor_persist_interval {
                        self.evict_stalled(&mut progress.outstanding);
                        let watermark = progress
                            .outstanding
                            .first_entry()
                            .map(|e| e.key().prev())
                            .unwrap_or(progress.last_seq);
                        if watermark.is_non_zero() {
                            tracing::trace!(seq = ?watermark, "persist cursor");
                            self.persist_cursor(watermark).await?;
                        }
                        progress.cursor_tick = Instant::now();
                    }
                }
            }
        }
    }

    async fn connect(
        &self,
        cursor: Option<Seq>,
    ) -> Result<SubscriptionStream<TolerantSubscribeReposStream>, FirehoseError<S::Error>> {
        let base = self.host.jacquard_uri_base("wss").map_err(|e| {
            counter!(FIREHOSE_CONNECTS_TOTAL, "result" => "failed").increment(1);
            FirehoseError::StreamSetupError(format!("invalid uri: {e}"))
        })?;
        let jacquard_cursor = cursor.map(|c| c.0 as i64);
        let params = TolerantSubscribeRepos {
            cursor: jacquard_cursor,
        };
        let client = TungsteniteClient::new();
        let stream = client
            .subscription(base.to_owned())
            .subscribe(&params)
            .await
            .map_err(|e| {
                counter!(FIREHOSE_CONNECTS_TOTAL, "result" => "failed").increment(1);
                FirehoseError::StreamSetupError(format!("subscribe: {e:?}"))
            })?;
        counter!(FIREHOSE_CONNECTS_TOTAL, "result" => "success").increment(1);
        Ok(stream)
    }

    async fn load_cursor(&self) -> Result<Option<CursorState>, FirehoseError<S::Error>> {
        let storage = self.storage.clone();
        let host = self.host.clone();
        tokio::task::spawn_blocking(move || CursorState::load(&storage, &host))
            .await
            .expect("db read not to panic")
            .map_err(FirehoseError::StorageLoad)
    }

    async fn persist_cursor(&self, seq: Seq) -> Result<(), FirehoseError<S::Error>> {
        let storage = self.storage.clone();
        let host = self.host.clone();
        tokio::task::spawn_blocking(move || {
            let cs = CursorState {
                seq,
                persisted_at: std::time::SystemTime::now(),
            };
            let mut batch = storage.batch();
            cs.store(&host, &mut batch);
            batch.commit()
        })
        .await
        .expect("db cursor persist not to panic")
        .map_err(FirehoseError::Storage)?;
        gauge!(FIREHOSE_CURSOR_SEQ).set(seq.0 as f64);
        Ok(())
    }

    fn evict_stalled(&self, outstanding: &mut BTreeMap<Seq, Instant>) {
        let threshold = Instant::now() - self.config.cursor_unack_hold_limit;
        let mut num_evicted = 0u64;
        outstanding.retain(|&seq, when| {
            if *when < threshold {
                warn!(seq = ?seq, host = %self.host.name(), "evicting stalled seq");
                num_evicted += 1;
                false
            } else {
                true
            }
        });
        if num_evicted > 0 {
            counter!(FIREHOSE_STALL_EVICTIONS_TOTAL).increment(num_evicted);
            gauge!(FIREHOSE_OUTSTANDING).set(outstanding.len() as f64);
        }
    }

    /// from a firehose event -> the right repo actor
    pub(super) async fn dispatch(
        &self,
        msg: SubscribeReposMessage,
        now: SystemTime,
        ack_tx: &mpsc::UnboundedSender<Seq>,
        cancel: &CancellationToken,
    ) -> Result<Option<Seq>, FirehoseError<S::Error>> {
        let Some((did, seq, time, prevalidated)) = map_message(msg).await else {
            return Ok(None);
        };

        // event time lag: how far behind live the stream position is.
        //
        // during replay, we can be far behind; this gague tracks our catch-up
        //
        // sets for every event received. can go negative if the uptream clock
        // is ahead of us or sending future times for any reason.
        let lag_seconds = match now.duration_since(time) {
            Ok(behind) => behind.as_secs_f64(),
            Err(ahead) => -ahead.duration().as_secs_f64(),
        };
        gauge!(FIREHOSE_UPSTREAM_LAG_SECONDS).set(lag_seconds);

        let payload = match prevalidated {
            Ok(m) => m,
            Err(err) => {
                counter!(FIREHOSE_VALIDATE_FAIL_TOTAL, "stage" => "pre").increment(1);
                debug!(%err, ?did, ?seq, "dropping prevalidate-failing event");
                return Ok(None);
            }
        };

        let event = FirehoseEvent {
            host: self.host.clone(),
            seq,
            time: now,
            upstream_time: time,
            payload,
            ack: FirehoseAck::new(seq, ack_tx.clone()),
        };

        let mut task = RepoMessage::Firehose(event);
        let mut backpressure_retries = 0;
        loop {
            // TODO: check cancellation token, otherwise we could spin here up to 2s
            // ... but also this should usually not block at all, so maybe skip the check
            match self.registry.try_send(&did, task) {
                Ok(()) => return Ok(Some(seq)),
                Err(RepoSendError::Backpressure(rejected)) => {
                    counter!(FIREHOSE_REPO_BACKPRESSURE_RETRIES_TOTAL).increment(1);
                    backpressure_retries += 1;
                    if backpressure_retries >= self.config.repo_backpressure_max_retries {
                        counter!(FIREHOSE_REPO_BACKPRESSURE_EXHAUSTED_TOTAL).increment(1);
                        return Err(FirehoseError::RepoBackpressureExhausted(did));
                    }
                    tokio::time::sleep(self.config.repo_backpressure_retry_sleep).await;
                    task = rejected;
                }
                Err(RepoSendError::Draining(_)) => {
                    if !cancel.is_cancelled() {
                        warn!(
                            "repo registry errored 'Draining' but our cancellation token is still alive. cancelling it."
                        );
                        cancel.cancel();
                    } else {
                        debug!("repo registry draining: dropping firehose message");
                    }
                    return Err(FirehoseError::Cancelled);
                }
                Err(RepoSendError::EvictionStuck(_)) => {
                    return Err(FirehoseError::RepoEvictionStuck);
                }
            }
        }
    }
}

enum ConnExit {
    Cancelled,
    Reconnect,
    ReconnectNow,
}

async fn map_message(
    m: SubscribeReposMessage,
) -> Option<(
    Did,
    Seq,
    SystemTime,
    Result<FirehosePayload, FirehoseValidationError>,
)> {
    use SubscribeReposMessage as M;

    let from_i = |s: i64| match u64::try_from(s) {
        Ok(u) => Some(Seq(u)),
        Err(_) => {
            info!(seq = %s, "dropping event with invalid seq");
            counter!(FIREHOSE_INVALID_SEQ_TOTAL).increment(1);
            None
        }
    };

    let from_dt = |dt: &Datetime| UNIX_EPOCH + Duration::from_millis(dt.timestamp_millis() as u64);

    match m {
        M::Commit(c) => {
            let seq = from_i(c.seq)?;
            let t = from_dt(&c.time);
            let did = (&c.repo).into();
            counter!(FIREHOSE_MESSAGES_TOTAL, "kind" => "commit").increment(1);
            trace!(did = %c.repo, rev = %c.rev, seq = ?seq, "firehose #commit");

            let vc = FirehoseCommit::prevalidate(*c)
                .await
                .map(|c| FirehosePayload::Commit(Box::new(c)));

            Some((did, seq, t, vc))
        }
        M::Sync(s) => {
            let seq = from_i(s.seq)?;
            let t = from_dt(&s.time);
            let did = (&s.did).into();
            counter!(FIREHOSE_MESSAGES_TOTAL, "kind" => "sync").increment(1);
            trace!(did = %s.did, rev = %s.rev, seq = ?seq, "firehose #sync");

            let vs = FirehoseSync::prevalidate(*s)
                .await
                .map(|s| FirehosePayload::Sync(Box::new(s)));

            Some((did, seq, t, vs))
        }
        M::Account(a) => {
            let seq = from_i(a.seq)?;
            let t = from_dt(&a.time);
            counter!(FIREHOSE_MESSAGES_TOTAL, "kind" => "account").increment(1);
            trace!(did = %a.did, active = a.active, seq = ?seq, "firehose #account");
            Some(((&a.did).into(), seq, t, Ok(FirehosePayload::Account(a))))
        }
        M::Identity(i) => {
            let seq = from_i(i.seq)?;
            let t = from_dt(&i.time);
            counter!(FIREHOSE_MESSAGES_TOTAL, "kind" => "identity").increment(1);
            trace!(did = %i.did, seq = ?seq, "firehose #identity");
            Some(((&i.did).into(), seq, t, Ok(FirehosePayload::Identity(i))))
        }
        M::Info(info) => {
            counter!(FIREHOSE_MESSAGES_TOTAL, "kind" => "info").increment(1);
            info!(name = %info.name, message = ?info.message, "firehose #info");
            None
        }
        M::Unknown(data) => {
            counter!(FIREHOSE_MESSAGES_TOTAL, "kind" => "unknown").increment(1);
            trace!(data = ?data, "firehose #[unknown]");
            None
        }
    }
}
