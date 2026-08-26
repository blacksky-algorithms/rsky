//! The daemon's run loop: keep a space credential fresh, sweep the writer set
//! (proposal §The sync boundary), react to write notifications, and fall back
//! to full-state recovery when incremental sync cannot proceed.

use rsky_lexicon::com::atproto::space::RepoRef;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::credentials::CredentialSource;
use crate::engine::{sync_repo, CommitKeyResolver, SyncOutcome};
use crate::error::{DaemonError, Result};
use crate::feeds::SpaceLifecycleAcker;
use crate::index::SpaceIndex;
use crate::journal::{drain_all, drain_all_sweep, SharedJournalConsumer};
use crate::notify::WriteNotice;
use crate::recovery::recover_repo;
use crate::repohost::RepoHostClient;
use crate::spaces::{SpaceRegistry, SpaceSource};
use crate::xrpc::SpaceHostClient;

const REGISTER_RETRY_SECS: u64 = 30;
const MIN_REREGISTER_SECS: u64 = 30;
/// How often pending batches are retried when no sync has happened, so a
/// destination that was down recovers without waiting for the next write.
const PROJECTION_DRAIN_SECS: u64 = 30;

/// Builds a repo-host client bound to the current space credential.
pub type RepoHostFactory = Box<dyn Fn(String) -> Arc<dyn RepoHostClient> + Send + Sync>;
pub type SpaceWorkerParts = (
    Arc<dyn CredentialSource>,
    RepoHostFactory,
    Arc<dyn SpaceIndex>,
    Vec<SharedJournalConsumer>,
    Option<Arc<dyn SpaceLifecycleAcker>>,
);
pub type MultiSpaceFactory = Arc<dyn Fn(&str) -> Result<SpaceWorkerParts> + Send + Sync>;

pub struct MultiRunnerOptions {
    pub refresh_interval_secs: u64,
    pub sweep_interval_secs: u64,
    pub notify_endpoint: String,
    pub service_identity: String,
    pub now_fn: fn() -> u64,
}

/// Supervises one existing `run` worker per discovered space. Source errors
/// retain the current worker set; a transient managing-app outage must not
/// silently stop private-feed projection.
#[allow(clippy::too_many_arguments)]
pub async fn run_multi(
    opts: MultiRunnerOptions,
    source: Arc<dyn SpaceSource>,
    registry: SpaceRegistry,
    factory: MultiSpaceFactory,
    host: Arc<dyn SpaceHostClient>,
    keys: Arc<dyn CommitKeyResolver>,
    mut notices: mpsc::Receiver<WriteNotice>,
    mut shutdown: watch::Receiver<bool>,
) {
    struct Worker {
        generation: i64,
        stop: watch::Sender<bool>,
        notices: mpsc::Sender<WriteNotice>,
        handle: tokio::task::JoinHandle<()>,
    }
    let mut workers: HashMap<String, Worker> = HashMap::new();
    let mut refresh = tokio::time::interval(Duration::from_secs(opts.refresh_interval_secs.max(1)));
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = refresh.tick() => {
                let desired = match source.spaces().await { Ok(value) => value, Err(error) => { tracing::warn!(error = %error, "space source unavailable; keeping current workers"); continue; } };
                let stale: Vec<_> = workers.iter().filter(|(space, worker)| desired.get(*space).is_none_or(|target| target.generation != worker.generation)).map(|(space, _)| space.clone()).collect();
                for space in stale { if let Some(worker) = workers.remove(&space) { let _ = worker.stop.send(true); let _ = worker.handle.await; } }
                for (space, target) in &desired { if workers.contains_key(space) { continue; }
                    let (creds, repo, index, projectors, acker) = match factory(space) { Ok(parts) => parts, Err(error) => { tracing::warn!(%space, error = %error, "cannot prepare space worker"); continue; } };
                    let (tx, rx) = mpsc::channel(256); let (stop, stop_rx) = watch::channel(false);
                    let worker_opts = RunnerOptions { space_uri: space.clone(), sweep_interval_secs: opts.sweep_interval_secs, notify_endpoint: opts.notify_endpoint.clone(), service_identity: opts.service_identity.clone(), generation: target.generation, now_fn: opts.now_fn };
                    let handle = tokio::spawn(run(worker_opts, host.clone(), creds, repo, index, keys.clone(), projectors, acker, rx, stop_rx));
                    workers.insert(space.clone(), Worker { generation: target.generation, stop, notices: tx, handle });
                }
                registry.replace(workers.keys().cloned().collect());
            }
            Some(notice) = notices.recv() => { if let Some(worker) = workers.get(&notice.0) { let _ = worker.notices.send(notice).await; } else { tracing::warn!(space = %notice.0, "notice for a space we do not sync"); } }
        }
    }
    for (space, worker) in workers {
        let _ = worker.stop.send(true);
        let _ = worker.handle.await;
        tracing::info!(%space, "stopped");
    }
    registry.replace(Default::default());
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SweepReport {
    pub synced: usize,
    pub skipped: usize,
    pub recovered: usize,
}

/// Sync one repo, falling back to full-state recovery on divergence or when
/// the host's oplog no longer covers our `since` revision.
pub async fn sync_repo_healing(
    client: &dyn RepoHostClient,
    index: &dyn SpaceIndex,
    keys: &dyn CommitKeyResolver,
    space_uri: &str,
    did: &str,
) -> Result<SyncOutcome> {
    match sync_repo(client, index, keys, space_uri, did).await {
        Err(e @ (DaemonError::Diverged(_) | DaemonError::HistoryUnavailable(_))) => {
            tracing::warn!(did, error = %e, "incremental sync failed; full-state recovery");
            recover_repo(client, index, keys, space_uri, did).await
        }
        other => other,
    }
}

async fn is_current(index: &dyn SpaceIndex, repo: &RepoRef) -> Result<bool> {
    let Some(rev) = index.last_rev(&repo.did).await? else {
        return Ok(false);
    };
    if rev != repo.rev {
        return Ok(false);
    }
    match &repo.hash {
        Some(hash) => {
            let ours = hex::encode(index.load_lthash(&repo.did).await?.hash());
            Ok(*hash == ours)
        }
        None => Ok(true),
    }
}

/// One full pass over the writer set: sync every repo whose `(rev, hash)` head
/// differs from ours, recovering any that cannot advance incrementally.
pub async fn sync_space_once(
    host: &dyn SpaceHostClient,
    client: &dyn RepoHostClient,
    index: &dyn SpaceIndex,
    keys: &dyn CommitKeyResolver,
    space_uri: &str,
    credential: &str,
) -> Result<SweepReport> {
    let mut report = SweepReport::default();
    let mut cursor: Option<String> = None;
    loop {
        let page = host
            .list_repos(space_uri, credential, cursor.as_deref(), None)
            .await?;
        for repo in &page.repos {
            if is_current(index, repo).await? {
                report.skipped += 1;
                continue;
            }
            match sync_repo(client, index, keys, space_uri, &repo.did).await {
                Ok(_) => report.synced += 1,
                Err(e @ (DaemonError::Diverged(_) | DaemonError::HistoryUnavailable(_))) => {
                    tracing::warn!(did = %repo.did, error = %e, "incremental sync failed; full-state recovery");
                    recover_repo(client, index, keys, space_uri, &repo.did).await?;
                    report.recovered += 1;
                }
                Err(e) => return Err(e),
            }
        }
        match page.cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(report)
}

pub struct RunnerOptions {
    pub space_uri: String,
    pub sweep_interval_secs: u64,
    pub notify_endpoint: String,
    /// This syncer's own service identifier, so the host can address its
    /// deliveries to it.
    pub service_identity: String,
    /// The space's generation as the managing app reported it; an
    /// acknowledgement names the generation it observed.
    pub generation: i64,
    pub now_fn: fn() -> u64,
}

async fn register(
    opts: &RunnerOptions,
    host: &dyn SpaceHostClient,
    creds: &dyn CredentialSource,
) -> Duration {
    let now = (opts.now_fn)();
    let attempt = async {
        let credential = creds.credential(now).await?;
        host.register_notify(
            &opts.space_uri,
            &credential,
            &opts.notify_endpoint,
            Some(&opts.service_identity),
        )
        .await
    };
    match attempt.await {
        Ok(expiry) => {
            let ttl = (expiry.timestamp().max(0) as u64).saturating_sub(now);
            // Re-register at 80% of the registration window.
            let renew = (ttl * 4 / 5).max(MIN_REREGISTER_SECS);
            tracing::info!(expiry = %expiry, renew_secs = renew, "notify registration active");
            Duration::from_secs(renew)
        }
        Err(e) => {
            tracing::warn!(error = %e, "notify registration failed; retrying");
            Duration::from_secs(REGISTER_RETRY_SECS)
        }
    }
}

async fn sweep(
    opts: &RunnerOptions,
    host: &dyn SpaceHostClient,
    creds: &dyn CredentialSource,
    make_repo_host: &RepoHostFactory,
    index: &dyn SpaceIndex,
    keys: &dyn CommitKeyResolver,
) -> bool {
    let attempt = async {
        let credential = creds.credential((opts.now_fn)()).await?;
        let client = make_repo_host(credential.clone());
        sync_space_once(
            host,
            client.as_ref(),
            index,
            keys,
            &opts.space_uri,
            &credential,
        )
        .await
    };
    match attempt.await {
        Ok(r) => {
            tracing::info!(synced = %r.synced, recovered = %r.recovered, "sweep complete");
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "sweep failed");
            false
        }
    }
}

async fn handle_notice(
    opts: &RunnerOptions,
    creds: &dyn CredentialSource,
    make_repo_host: &RepoHostFactory,
    index: &dyn SpaceIndex,
    keys: &dyn CommitKeyResolver,
    notice: WriteNotice,
) {
    let (space, did) = notice;
    if space != opts.space_uri {
        tracing::warn!(space = %space, "dropping notice for space we do not sync");
        return;
    }
    let attempt = async {
        let credential = creds.credential((opts.now_fn)()).await?;
        let client = make_repo_host(credential);
        sync_repo_healing(client.as_ref(), index, keys, &opts.space_uri, &did).await
    };
    match attempt.await {
        Ok(o) => tracing::debug!(did, verified = %o.commit_verified, "notified repo synced"),
        Err(e) => tracing::warn!(did, error = %e, "notified repo sync failed"),
    }
}

/// The daemon loop: register for notifications (re-registering before
/// expiry), consume the notify queue, and sweep the writer set on an interval
/// as the self-healing path for dropped notifications.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    opts: RunnerOptions,
    host: Arc<dyn SpaceHostClient>,
    creds: Arc<dyn CredentialSource>,
    make_repo_host: RepoHostFactory,
    index: Arc<dyn SpaceIndex>,
    keys: Arc<dyn CommitKeyResolver>,
    projectors: Vec<SharedJournalConsumer>,
    lifecycle_acker: Option<Arc<dyn SpaceLifecycleAcker>>,
    mut notify_rx: mpsc::Receiver<WriteNotice>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut sweep_timer =
        tokio::time::interval(Duration::from_secs(opts.sweep_interval_secs.max(1)));
    let mut drain_timer = tokio::time::interval(Duration::from_secs(PROJECTION_DRAIN_SECS));
    let mut register_at = Instant::now();
    let mut acknowledged = false;
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                tracing::info!("runner shutting down");
                return;
            }
            _ = tokio::time::sleep_until(register_at) => {
                register_at = Instant::now()
                    + register(&opts, host.as_ref(), creds.as_ref()).await;
            }
            _ = sweep_timer.tick() => {
                let swept = sweep(
                    &opts,
                    host.as_ref(),
                    creds.as_ref(),
                    &make_repo_host,
                    index.as_ref(),
                    keys.as_ref(),
                )
                .await;
                let projected = drain_all_sweep(index.as_ref(), &projectors).await;
                if swept && projected && !acknowledged {
                    if let Some(acker) = &lifecycle_acker {
                        match acker.acknowledge_sync(&opts.space_uri, opts.generation).await {
                            Ok(()) => acknowledged = true,
                            Err(error) => tracing::warn!(error = %error, "space lifecycle acknowledgement failed"),
                        }
                    }
                }
            }
            _ = drain_timer.tick() => {
                drain_all(index.as_ref(), &projectors).await;
            }
            Some(notice) = notify_rx.recv() => {
                handle_notice(
                    &opts,
                    creds.as_ref(),
                    &make_repo_host,
                    index.as_ref(),
                    keys.as_ref(),
                    notice,
                )
                .await;
                drain_all(index.as_ref(), &projectors).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::StaticCredential;
    use crate::index::InMemoryIndex;
    use crate::recovery::tests::{author, fixture, Author, FixedKey, Fixture, AUTHOR, SPACE};
    use crate::repohost::OplogPage;
    use async_trait::async_trait;
    use chrono::{DateTime, TimeZone, Utc};
    use rsky_lexicon::com::atproto::space::ListReposOutput;
    use rsky_space::car::repo_car_bytes;
    use rsky_space::commit::{build_ctx, compute_mac};
    use rsky_space::lthash::{element, LtHash};
    use rsky_space::types::{RepoOp, SignedCommit};
    use secp256k1::Message;
    use serde_bytes::ByteBuf;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const COLL: &str = "community.blacksky.feed.post";

    /// A real subscriber so multi-line tracing macro fields are evaluated.
    fn trace_guard() -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .finish(),
        )
    }

    /// A commit bound to an arbitrary author did (the recovery fixture pins
    /// AUTHOR; the sweep scenarios need several writers).
    fn commit_as(
        a: &Author,
        elements: &[(String, String, String)],
        rev: &str,
        did: &str,
    ) -> SignedCommit {
        let mut lth = LtHash::new();
        for (c, r, cid) in elements {
            lth.add(&element(c, r, cid));
        }
        let hash = lth.hash();
        let ikm = [9u8; 32];
        let ctx = build_ctx(SPACE, did, rev, &ikm);
        let digest = Sha256::digest(&ctx);
        let msg = Message::from_digest_slice(&digest).unwrap();
        let mut sig = a.secret.sign_ecdsa(msg);
        sig.normalize_s();
        let mac = compute_mac(&ikm, &ctx, &hash).unwrap();
        SignedCommit {
            ver: 1,
            hash: ByteBuf::from(hash.to_vec()),
            ikm: ByteBuf::from(ikm.to_vec()),
            sig: ByteBuf::from(sig.serialize_compact().to_vec()),
            mac: ByteBuf::from(mac.to_vec()),
            rev: rev.to_string(),
        }
    }

    async fn car_as(f: &Fixture, did: &str, rev: &str) -> Vec<u8> {
        let elements: Vec<(String, String, String)> = f
            .entries
            .iter()
            .map(|(path, cid)| {
                let (c, r) = path.split_once('/').unwrap();
                (c.to_string(), r.to_string(), cid.to_string())
            })
            .collect();
        let commit = commit_as(&f.author, &elements, rev, did);
        let store = f.store.clone();
        repo_car_bytes(&commit, &f.entries, move |cid| store.get(cid).cloned())
            .await
            .unwrap()
    }

    enum Repo {
        Oplog(OplogPage),
        HistoryGone { car: Vec<u8> },
        DivergedOplog { page: OplogPage, car: Vec<u8> },
    }

    struct ScriptedRepoHost(HashMap<String, Repo>);
    impl ScriptedRepoHost {
        fn repo(&self, did: &str) -> Result<&Repo> {
            self.0
                .get(did)
                .ok_or_else(|| DaemonError::Xrpc(format!("unknown repo {did}")))
        }
    }
    #[async_trait]
    impl RepoHostClient for ScriptedRepoHost {
        async fn list_repo_ops(
            &self,
            _space: &str,
            did: &str,
            _since: Option<&str>,
            _cursor: Option<&str>,
        ) -> Result<OplogPage> {
            match self.repo(did)? {
                Repo::Oplog(page) | Repo::DivergedOplog { page, .. } => Ok(OplogPage {
                    ops: page.ops.clone(),
                    commit: page.commit.clone(),
                    cursor: None,
                }),
                Repo::HistoryGone { .. } => Err(DaemonError::HistoryUnavailable(
                    "oplog compacted".to_string(),
                )),
            }
        }
        async fn get_repo_car(&self, _space: &str, did: &str) -> Result<Vec<u8>> {
            match self.repo(did)? {
                Repo::HistoryGone { car } | Repo::DivergedOplog { car, .. } => Ok(car.clone()),
                Repo::Oplog(_) => Err(DaemonError::Xrpc("no car scripted".to_string())),
            }
        }
        async fn get_latest_commit(&self, _space: &str, _did: &str) -> Result<SignedCommit> {
            Err(DaemonError::Xrpc("unused".to_string()))
        }
    }

    struct BrokenHost;
    #[async_trait]
    impl RepoHostClient for BrokenHost {
        async fn list_repo_ops(
            &self,
            _space: &str,
            _did: &str,
            _since: Option<&str>,
            _cursor: Option<&str>,
        ) -> Result<OplogPage> {
            Err(DaemonError::Xrpc("boom".to_string()))
        }
        async fn get_repo_car(&self, _space: &str, _did: &str) -> Result<Vec<u8>> {
            Err(DaemonError::Xrpc("boom".to_string()))
        }
        async fn get_latest_commit(&self, _space: &str, _did: &str) -> Result<SignedCommit> {
            Err(DaemonError::Xrpc("boom".to_string()))
        }
    }

    struct PagedSpaceHost {
        pages: Vec<ListReposOutput>,
        registrations: AtomicUsize,
        expiry: DateTime<Utc>,
    }
    impl PagedSpaceHost {
        fn new(pages: Vec<ListReposOutput>) -> Self {
            Self {
                pages,
                registrations: AtomicUsize::new(0),
                expiry: Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap(),
            }
        }
    }
    #[async_trait]
    impl SpaceHostClient for PagedSpaceHost {
        async fn get_space_credential(
            &self,
            _space: &str,
            _delegation_token: &str,
            _client_attestation: Option<&str>,
        ) -> Result<String> {
            Ok("sc.jwt".to_string())
        }
        async fn list_repos(
            &self,
            space: &str,
            credential: &str,
            cursor: Option<&str>,
            _limit: Option<i64>,
        ) -> Result<ListReposOutput> {
            assert_eq!(space, SPACE);
            assert_eq!(credential, "sc.jwt");
            let i: usize = cursor.map(|c| c.parse().unwrap()).unwrap_or(0);
            let mut page = self.pages[i].clone();
            page.cursor = (i + 1 < self.pages.len()).then(|| (i + 1).to_string());
            Ok(page)
        }
        async fn register_notify(
            &self,
            _space: &str,
            _credential: &str,
            _endpoint: &str,
            _service: Option<&str>,
        ) -> Result<DateTime<Utc>> {
            self.registrations.fetch_add(1, Ordering::SeqCst);
            Ok(self.expiry)
        }
    }

    fn repo_ref(
        did: &str,
        rev: &str,
        hash: Option<String>,
    ) -> rsky_lexicon::com::atproto::space::RepoRef {
        rsky_lexicon::com::atproto::space::RepoRef {
            did: did.to_string(),
            rev: rev.to_string(),
            hash,
        }
    }

    fn oplog_repo(a: &Author, did: &str, rkey: &str, cid: &str, rev: &str) -> Repo {
        let commit = commit_as(
            a,
            &[(COLL.to_string(), rkey.to_string(), cid.to_string())],
            rev,
            did,
        );
        Repo::Oplog(OplogPage {
            ops: vec![RepoOp {
                rev: rev.to_string(),
                collection: COLL.to_string(),
                rkey: rkey.to_string(),
                cid: Some(cid.to_string()),
                prev: None,
                value: Some(ByteBuf::from(vec![1u8])),
            }],
            commit: Some(commit),
            cursor: None,
        })
    }

    /// Advanced repo synced, up-to-date repo skipped, diverged repo recovered,
    /// HistoryUnavailable repo recovered — across two writer-set pages.
    #[tokio::test]
    async fn sync_space_once_covers_all_repo_states() {
        let _guard = trace_guard();
        let a = author();
        let f = fixture();
        let index = InMemoryIndex::new();
        let keys = FixedKey(a.did_key.clone());

        // Up-to-date repo: head already saved, hash advertised by the host.
        let mut current_lth = LtHash::new();
        current_lth.add(&element(COLL, "3ka", "bafyCur"));
        index
            .upsert("did:plc:current", COLL, "3ka", "bafyCur", "3rev", None)
            .await
            .unwrap();
        index
            .save_head("did:plc:current", "3rev", &current_lth)
            .await
            .unwrap();
        let current_hash = hex::encode(current_lth.hash());
        // Up to date by rev alone: the host advertises no hash.
        index
            .save_head("did:plc:hashless", "3rev", &LtHash::new())
            .await
            .unwrap();
        // A stale head rev: must resync.
        index
            .save_head("did:plc:stale", "3rev1", &LtHash::new())
            .await
            .unwrap();

        let mut repos = HashMap::new();
        repos.insert(
            "did:plc:stale".to_string(),
            oplog_repo(&a, "did:plc:stale", "3ks", "bafyS", "3rev2"),
        );
        repos.insert(
            "did:plc:advanced".to_string(),
            oplog_repo(&a, "did:plc:advanced", "3kb", "bafyNew", "3rev2"),
        );
        repos.insert(
            "did:plc:diverged".to_string(),
            Repo::DivergedOplog {
                page: OplogPage {
                    ops: vec![RepoOp {
                        rev: "3rev2".to_string(),
                        collection: COLL.to_string(),
                        rkey: "3kz".to_string(),
                        cid: Some("bafyWrong".to_string()),
                        prev: None,
                        value: None,
                    }],
                    commit: match oplog_repo(&a, "did:plc:diverged", "3kb", "bafyNew", "3rev2") {
                        Repo::Oplog(p) => p.commit,
                        _ => unreachable!(),
                    },
                    cursor: None,
                },
                car: car_as(&f, "did:plc:diverged", "3rev2").await,
            },
        );
        repos.insert(
            "did:plc:gone".to_string(),
            Repo::HistoryGone {
                car: car_as(&f, "did:plc:gone", "3rev2").await,
            },
        );
        let client = ScriptedRepoHost(repos);

        let host = PagedSpaceHost::new(vec![
            ListReposOutput {
                cursor: None,
                repos: vec![
                    repo_ref("did:plc:current", "3rev", Some(current_hash)),
                    repo_ref("did:plc:hashless", "3rev", None),
                    repo_ref("did:plc:stale", "3rev2", None),
                    repo_ref("did:plc:advanced", "3rev2", None),
                ],
            },
            ListReposOutput {
                cursor: None,
                repos: vec![
                    repo_ref("did:plc:diverged", "3rev2", None),
                    repo_ref("did:plc:gone", "3rev2", None),
                ],
            },
        ]);

        assert_eq!(
            host.get_space_credential(SPACE, "dt.jwt", None)
                .await
                .unwrap(),
            "sc.jwt"
        );
        let report = sync_space_once(&host, &client, &index, &keys, SPACE, "sc.jwt")
            .await
            .unwrap();
        assert_eq!(
            report,
            SweepReport {
                synced: 2,
                skipped: 2,
                recovered: 2,
            }
        );
        assert_eq!(index.record_count("did:plc:stale"), 1);
        assert_eq!(index.record_count("did:plc:advanced"), 1);
        assert_eq!(index.record_count("did:plc:diverged"), 3);
        assert_eq!(index.record_count("did:plc:gone"), 3);
        assert_eq!(
            index.last_rev("did:plc:current").await.unwrap().as_deref(),
            Some("3rev")
        );
    }

    #[tokio::test]
    async fn sync_space_once_hash_mismatch_forces_resync() {
        let a = author();
        let index = InMemoryIndex::new();
        let keys = FixedKey(a.did_key.clone());
        // Head rev matches the writer set but the advertised hash does not:
        // the repo advanced without us seeing it, so it must resync.
        let mut lth = LtHash::new();
        lth.add(&element(COLL, "3ka", "bafyOld"));
        index
            .upsert("did:plc:writer", COLL, "3ka", "bafyOld", "3rev2", None)
            .await
            .unwrap();
        index
            .save_head("did:plc:writer", "3rev2", &lth)
            .await
            .unwrap();

        let commit = commit_as(
            &a,
            &[
                (COLL.to_string(), "3ka".to_string(), "bafyOld".to_string()),
                (COLL.to_string(), "3kb".to_string(), "bafyNew".to_string()),
            ],
            "3rev3",
            "did:plc:writer",
        );
        let mut repos = HashMap::new();
        repos.insert(
            "did:plc:writer".to_string(),
            Repo::Oplog(OplogPage {
                ops: vec![RepoOp {
                    rev: "3rev3".to_string(),
                    collection: COLL.to_string(),
                    rkey: "3kb".to_string(),
                    cid: Some("bafyNew".to_string()),
                    prev: None,
                    value: None,
                }],
                commit: Some(commit),
                cursor: None,
            }),
        );
        let client = ScriptedRepoHost(repos);
        let host = PagedSpaceHost::new(vec![ListReposOutput {
            cursor: None,
            repos: vec![repo_ref(
                "did:plc:writer",
                "3rev2",
                Some("ffff".to_string()),
            )],
        }]);

        let report = sync_space_once(&host, &client, &index, &keys, SPACE, "sc.jwt")
            .await
            .unwrap();
        assert_eq!(report.synced, 1);
        assert_eq!(
            index.last_rev("did:plc:writer").await.unwrap().as_deref(),
            Some("3rev3")
        );
    }

    #[tokio::test]
    async fn sync_space_once_propagates_unrecoverable_errors() {
        let a = author();
        let index = InMemoryIndex::new();
        let keys = FixedKey(a.did_key.clone());

        let host = PagedSpaceHost::new(vec![ListReposOutput {
            cursor: None,
            repos: vec![repo_ref("did:plc:writer", "3rev", None)],
        }]);
        let err = sync_space_once(&host, &BrokenHost, &index, &keys, SPACE, "sc.jwt")
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(_)));
        assert!(BrokenHost.get_repo_car(SPACE, AUTHOR).await.is_err());
        assert!(BrokenHost.get_latest_commit(SPACE, AUTHOR).await.is_err());
    }

    #[tokio::test]
    async fn sync_repo_healing_passes_through_and_recovers() {
        let _guard = trace_guard();
        let a = author();
        let f = fixture();
        let keys = FixedKey(a.did_key.clone());

        // Pass-through success.
        let index = InMemoryIndex::new();
        let mut repos = HashMap::new();
        repos.insert(
            AUTHOR.to_string(),
            oplog_repo(&a, AUTHOR, "3ka", "bafyA", "3rev"),
        );
        let client = ScriptedRepoHost(repos);
        let outcome = sync_repo_healing(&client, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert!(outcome.commit_verified);
        // An Oplog-only script has no CAR; unused stubs exercised for coverage.
        assert!(client.get_repo_car(SPACE, AUTHOR).await.is_err());
        assert!(client.get_latest_commit(SPACE, AUTHOR).await.is_err());

        // HistoryUnavailable falls back to recovery.
        let index = InMemoryIndex::new();
        let mut repos = HashMap::new();
        repos.insert(
            AUTHOR.to_string(),
            Repo::HistoryGone {
                car: car_as(&f, AUTHOR, "3rev9").await,
            },
        );
        let client = ScriptedRepoHost(repos);
        let outcome = sync_repo_healing(&client, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert!(outcome.commit_verified);
        assert_eq!(index.record_count(AUTHOR), 3);

        // Non-recoverable errors pass through unchanged.
        let index = InMemoryIndex::new();
        let err = sync_repo_healing(&BrokenHost, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(_)));
    }

    fn fixed_now() -> u64 {
        1_700_000_000
    }

    fn options(sweep_secs: u64) -> RunnerOptions {
        RunnerOptions {
            space_uri: SPACE.to_string(),
            sweep_interval_secs: sweep_secs,
            notify_endpoint: "https://syncer.example/notify".to_string(),
            service_identity: "did:web:syncer.example".to_string(),
            generation: 1,
            now_fn: fixed_now,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn run_registers_sweeps_and_consumes_notices() {
        let _guard = trace_guard();
        let a = author();
        let mut repos = HashMap::new();
        repos.insert(
            "did:plc:noticed".to_string(),
            oplog_repo(&a, "did:plc:noticed", "3ka", "bafyN", "3rev"),
        );
        let client: Arc<dyn RepoHostClient> = Arc::new(ScriptedRepoHost(repos));
        let host = Arc::new(PagedSpaceHost::new(vec![ListReposOutput {
            cursor: None,
            repos: vec![],
        }]));
        let index = Arc::new(InMemoryIndex::new());
        let keys = Arc::new(FixedKey(a.did_key.clone()));
        let creds = Arc::new(StaticCredential("sc.jwt".to_string()));
        let (tx, rx) = mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let factory_client = client.clone();
        let make_repo_host: RepoHostFactory = Box::new(move |credential| {
            assert_eq!(credential, "sc.jwt");
            factory_client.clone()
        });

        let handle = tokio::spawn(run(
            options(3600),
            host.clone(),
            creds,
            make_repo_host,
            index.clone(),
            keys,
            Vec::new(),
            None,
            rx,
            shutdown_rx,
        ));

        tx.send((SPACE.to_string(), "did:plc:noticed".to_string()))
            .await
            .unwrap();
        tx.send((
            "at://x/space/y/z".to_string(),
            "did:plc:ignored".to_string(),
        ))
        .await
        .unwrap();
        for _ in 0..200 {
            if index.record_count("did:plc:noticed") == 1
                && host.registrations.load(Ordering::SeqCst) >= 1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(index.record_count("did:plc:noticed"), 1);
        assert_eq!(index.record_count("did:plc:ignored"), 0);
        assert!(host.registrations.load(Ordering::SeqCst) >= 1);

        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_worker_drains_each_projector_independently() {
        use crate::feeds::tests::{post_created, CapturingIngress};
        use crate::feeds::FeedsProjector;
        use crate::index::IndexMutation;
        use crate::journal::JournalConsumer;
        use crate::projection::Projector;
        use crate::router::{Router, SyncEvent, POST_COLLECTION};
        use rsky_space::record::encode_record;
        use rsky_space::space_id::SpaceId;

        struct Stalled;
        #[async_trait]
        impl Projector for Stalled {
            fn name(&self) -> &'static str {
                "appview"
            }
            async fn project(&self, _did: &str, _rev: &str, _events: &[SyncEvent]) -> Result<()> {
                Err(DaemonError::RetryableProjection("down".to_string()))
            }
        }

        let space = SpaceId::parse(SPACE).unwrap();
        let router = || Router::new(space.clone(), space.authority.clone());
        let feeds = Arc::new(JournalConsumer::new(
            router(),
            Box::new(FeedsProjector::new(CapturingIngress::default(), SPACE)),
        ));
        let appview = Arc::new(JournalConsumer::new(router(), Box::new(Stalled)));

        let index = Arc::new(InMemoryIndex::new());
        index
            .journal_batch(
                AUTHOR,
                "3krev",
                &[IndexMutation::Upsert {
                    collection: POST_COLLECTION.to_string(),
                    rkey: "3ka".to_string(),
                    cid: "bafypost".to_string(),
                    rev: "3krev".to_string(),
                    value: Some(
                        encode_record(
                            &match post_created("3ka", "hello") {
                                SyncEvent::PostCreated { record, .. } => record,
                                _ => unreachable!("fixture is a post create"),
                            },
                            64 * 1024,
                        )
                        .unwrap(),
                    ),
                }],
            )
            .await
            .unwrap();

        let a = author();
        let host = Arc::new(PagedSpaceHost::new(vec![ListReposOutput {
            cursor: None,
            repos: vec![],
        }]));
        let client: Arc<dyn RepoHostClient> = Arc::new(ScriptedRepoHost(HashMap::new()));
        let make_repo_host: RepoHostFactory = Box::new(move |_| client.clone());
        let (_tx, rx) = mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = tokio::spawn(run(
            options(3600),
            host,
            Arc::new(StaticCredential("sc.jwt".to_string())),
            make_repo_host,
            index.clone(),
            Arc::new(FixedKey(a.did_key.clone())),
            vec![feeds.clone(), appview.clone()],
            None,
            rx,
            shutdown_rx,
        ));

        for _ in 0..200 {
            if index.pending_batches("feeds").await.unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(index.pending_batches("feeds").await.unwrap().is_empty());
        assert_eq!(index.pending_batches("appview").await.unwrap().len(), 1);

        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_clean_sweep_acknowledges_the_space_generation_once() {
        #[derive(Default)]
        struct RecordingAcker(std::sync::Mutex<Vec<(String, i64)>>);
        #[async_trait]
        impl SpaceLifecycleAcker for RecordingAcker {
            async fn acknowledge_sync(&self, space: &str, generation: i64) -> Result<()> {
                self.0.lock().unwrap().push((space.to_string(), generation));
                Ok(())
            }
        }

        let a = author();
        let host = Arc::new(PagedSpaceHost::new(vec![ListReposOutput {
            cursor: None,
            repos: vec![],
        }]));
        let client: Arc<dyn RepoHostClient> = Arc::new(ScriptedRepoHost(HashMap::new()));
        let make_repo_host: RepoHostFactory = Box::new(move |_| client.clone());
        let acker = Arc::new(RecordingAcker::default());
        let (_tx, rx) = mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = tokio::spawn(run(
            RunnerOptions {
                sweep_interval_secs: 1,
                generation: 7,
                ..options(1)
            },
            host,
            Arc::new(StaticCredential("sc.jwt".to_string())),
            make_repo_host,
            Arc::new(InMemoryIndex::new()),
            Arc::new(FixedKey(a.did_key.clone())),
            Vec::new(),
            Some(acker.clone()),
            rx,
            shutdown_rx,
        ));

        for _ in 0..200 {
            if !acker.0.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        assert_eq!(
            *acker.0.lock().unwrap(),
            vec![(SPACE.to_string(), 7_i64)],
            "the acknowledgement is sent once, not on every sweep"
        );
    }

    struct FailingSpaceHost;
    #[async_trait]
    impl SpaceHostClient for FailingSpaceHost {
        async fn get_space_credential(
            &self,
            _space: &str,
            _delegation_token: &str,
            _client_attestation: Option<&str>,
        ) -> Result<String> {
            Err(DaemonError::Xrpc("mint down".to_string()))
        }
        async fn list_repos(
            &self,
            _space: &str,
            _credential: &str,
            _cursor: Option<&str>,
            _limit: Option<i64>,
        ) -> Result<ListReposOutput> {
            Err(DaemonError::Xrpc("host down".to_string()))
        }
        async fn register_notify(
            &self,
            _space: &str,
            _credential: &str,
            _endpoint: &str,
            _service: Option<&str>,
        ) -> Result<DateTime<Utc>> {
            Err(DaemonError::Xrpc("host down".to_string()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn run_survives_host_failures_and_shuts_down() {
        let _guard = trace_guard();
        let a = author();
        let client: Arc<dyn RepoHostClient> = Arc::new(ScriptedRepoHost(HashMap::new()));
        let host = Arc::new(FailingSpaceHost);
        assert!(host.get_space_credential(SPACE, "dt", None).await.is_err());
        let index = Arc::new(InMemoryIndex::new());
        let keys = Arc::new(FixedKey(a.did_key.clone()));
        let creds = Arc::new(StaticCredential("sc.jwt".to_string()));
        let (tx, rx) = mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let factory_client = client.clone();
        let make_repo_host: RepoHostFactory = Box::new(move |_credential| factory_client.clone());

        let handle = tokio::spawn(run(
            options(1),
            host,
            creds,
            make_repo_host,
            index,
            keys,
            Vec::new(),
            None,
            rx,
            shutdown_rx,
        ));

        // A notice for a did the mock does not know: the sync fails and the
        // loop keeps running.
        tx.send((SPACE.to_string(), "did:plc:unknown".to_string()))
            .await
            .ok();
        // Let the register retry, failing sweep, and notice arms all fire.
        tokio::time::sleep(Duration::from_secs(120)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();
    }

    struct FailingCreds;
    #[async_trait]
    impl CredentialSource for FailingCreds {
        async fn credential(&self, _now: u64) -> Result<String> {
            Err(DaemonError::Xrpc("no credential".to_string()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn run_survives_credential_failures() {
        let _guard = trace_guard();
        let a = author();
        let client: Arc<dyn RepoHostClient> = Arc::new(ScriptedRepoHost(HashMap::new()));
        let host = Arc::new(PagedSpaceHost::new(vec![ListReposOutput {
            cursor: None,
            repos: vec![],
        }]));
        let index = Arc::new(InMemoryIndex::new());
        let keys = Arc::new(FixedKey(a.did_key.clone()));
        let (tx, rx) = mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let factory_client = client.clone();
        let make_repo_host: RepoHostFactory = Box::new(move |_credential| factory_client.clone());

        let handle = tokio::spawn(run(
            options(1),
            host,
            Arc::new(FailingCreds),
            make_repo_host,
            index,
            keys,
            Vec::new(),
            None,
            rx,
            shutdown_rx,
        ));

        tx.send((SPACE.to_string(), "did:plc:writer".to_string()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(120)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();
    }
}
