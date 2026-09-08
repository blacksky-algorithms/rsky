//! public view into hubble-sync repo contents

use std::marker::PhantomData;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::SystemTime;

use http::StatusCode;
use serde::Serialize;
use tokio::sync::oneshot;
use tracing::{debug, error, info};

use crate::storage::moderation_log::ModerateEvent;
use crate::storage::repo::{AccountStatus, AccountStatusEvent, ModAction, SyncStatus};
use crate::{
    Did, HostRegistry, LoadError, ModerateOutcome, PrefixedEngine, Repo, RepoContext,
    RepoCountsByState, RepoCountsByStatus, RepoMessage, RepoSendError, RepoSender, RepoSlot,
    StorageEngine, Tid,
};

/// read-only view into hubble-sync's state about repos
#[derive(Clone)]
pub struct SyncHandle<S: StorageEngine, C: RepoSlot, I: RepoSlot> {
    storage: PrefixedEngine<S>,
    hosts: Arc<HostRegistry>,
    endpoints: SyncEndpoints<PrefixedEngine<S>>,
    slots: PhantomData<(C, I)>,
}

impl<S: StorageEngine, C: RepoSlot, I: RepoSlot> SyncHandle<S, C, I> {
    pub(crate) fn new(
        storage: PrefixedEngine<S>,
        hosts: Arc<HostRegistry>,
        repo_sender: Arc<dyn RepoSender>,
    ) -> Self {
        let endpoints = SyncEndpoints::new(storage.clone(), hosts.clone(), repo_sender.clone());
        Self {
            storage,
            hosts,
            endpoints,
            slots: PhantomData,
        }
    }

    /// blocking(!) repo lookup by DID
    pub fn get(&self, did: &Did) -> Result<Option<RepoView<C, I>>, LoadError<S::Error>> {
        let Some(repo) = Repo::load(&self.storage, &self.hosts, did)? else {
            return Ok(None);
        };
        let (commit, info) = Repo::load_app_slots::<_, C, I>(did, &self.storage)?;
        Ok(Some(RepoView { repo, commit, info }))
    }

    pub fn endpoints(&self) -> SyncEndpoints<PrefixedEngine<S>> {
        self.endpoints.clone()
    }

    pub fn storage(&self) -> &S {
        self.storage.inner()
    }
}

#[derive(Debug)]
pub struct RepoView<C: RepoSlot, I: RepoSlot> {
    repo: Repo,
    commit: Option<C>,
    info: Option<I>,
}

impl<C: RepoSlot, I: RepoSlot> RepoView<C, I> {
    pub fn context(&self) -> RepoContext<'_> {
        RepoContext::new(&self.repo)
    }
    pub fn commit(&self) -> Option<&C> {
        self.commit.as_ref()
    }
    pub fn info(&self) -> Option<&I> {
        self.info.as_ref()
    }
}

/// handle for sync-generic interfaces
///
/// split from SyncHandle to drop the slot generics (sync-generic stuff can't
/// use app-owned slot data by definition).
#[derive(Clone)]
pub struct SyncEndpoints<S: StorageEngine> {
    storage: S,
    hosts: Arc<HostRegistry>,
    repo_sender: Arc<dyn RepoSender>,
}

impl<S: StorageEngine> SyncEndpoints<S> {
    pub(crate) fn new(
        storage: S,
        hosts: Arc<HostRegistry>,
        repo_sender: Arc<dyn RepoSender>,
    ) -> Self {
        Self {
            storage,
            hosts,
            repo_sender,
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn get_repo_status(&self, did: &Did) -> Result<GetRepoStatusResponse, EndpointError> {
        let repo = Repo::load(&self.storage, &self.hosts, did)
            .inspect_err(|err| error!(%did, %err, "failed to load repo"))
            .map_err(|_| EndpointError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                error: "InternalError",
                message: Some("failed to load repo"),
            })?
            .ok_or(EndpointError {
                status: StatusCode::BAD_REQUEST,
                error: "RepoNotFound",
                message: None,
            })?;

        let did = did.clone();

        let rev = repo.rev().ok_or(EndpointError {
            status: StatusCode::BAD_REQUEST,
            error: "RepoNotFound",
            message: Some("not yet synchronized (no rev)"),
        })?;

        let acct = &repo.info().account_status();
        let sync = &repo.info().sync_status;
        let (active, status) = self.atproto_account_status(acct, sync);

        Ok(GetRepoStatusResponse {
            did,
            rev,
            active,
            status,
        })
    }

    #[tracing::instrument(skip(self))]
    pub fn list_repos(
        &self,
        limit: NonZeroUsize,
        cursor: Option<&str>,
    ) -> Result<ListReposResponse, EndpointError> {
        let after = cursor.map(Did::new).transpose().map_err(|err| {
            debug!(%err, "unusable non-DID cursor");
            EndpointError {
                status: StatusCode::BAD_REQUEST,
                error: "InvalidRequest",
                message: Some("invalid cursor"),
            }
        })?;

        let repos = Repo::list_synced(&self.storage, &self.hosts, after.as_ref(), limit).map_err(
            |err| {
                error!(%err, "list_repos list_synced");
                EndpointError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    error: "InternalError",
                    message: Some("failed to list repos"),
                }
            },
        )?;

        let next_cursor = if repos.len() >= limit.into() {
            let (did, _, _) = repos.last().expect("non-zero len by limit");
            Some(did.to_string())
        } else {
            None
        };

        let repos = repos
            .into_iter()
            .map(|(did, info, sync_state)| {
                let acct = &info.account_status();
                let sync = &info.sync_status;
                let (active, status) = self.atproto_account_status(acct, sync);
                GetRepoStatusResponse {
                    did,
                    rev: sync_state.rev,
                    active,
                    status,
                }
            })
            .collect();

        Ok(ListReposResponse {
            repos,
            cursor: next_cursor,
        })
    }

    pub fn atproto_account_status(
        &self,
        acct: &AccountStatus,
        sync: &SyncStatus,
    ) -> (bool, Option<&'static str>) {
        use crate::storage::repo::DesyncReason;
        use {AccountStatus as A, SyncStatus as S};
        match (acct, sync) {
            (A::Active, S::Synchronized) => (true, None),
            (A::Active, S::OutOfScope { .. }) => (true, Some("outOfScope")),
            (A::Active, S::Desynchronized(d))
                if matches!(d.reason, DesyncReason::Throttled { .. }) =>
            {
                (true, Some("throttled"))
            }
            (A::Active, S::Desynchronized(_)) => (true, Some("desynchronized")),
            (A::Active, _) => (true, None),
            (A::Deleted, _) => (false, Some("deleted")),
            (A::Deactivated, _) => (false, Some("deactivated")),
            (A::Takendown, _) => (false, Some("takendown")),
            (A::Suspended, _) => (false, Some("suspended")),
            (A::Inactive(unrecognized_reason), _) => {
                info!(
                    reason = unrecognized_reason,
                    "returning 'other' for unrecognized inactive reason"
                );
                (false, Some("other"))
            }
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn repo_counts_by_status(&self) -> Result<RepoCountsByStatus, EndpointError> {
        RepoCountsByStatus::load(&self.storage).map_err(|err| {
            error!(%err, "repo counts");
            EndpointError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                error: "InternalError",
                message: Some("failed to get repo counts"),
            }
        })
    }

    #[tracing::instrument(skip(self))]
    pub fn repo_counts_by_state(&self) -> Result<RepoCountsByState, EndpointError> {
        RepoCountsByState::load(&self.storage).map_err(|err| {
            error!(%err, "account counts");
            EndpointError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                error: "InternalError",
                message: Some("failed to get account counts"),
            }
        })
    }

    /// moderate an account
    ///
    /// setting action to `None` will clear a previously applied moderation
    ///
    /// async!! does not need spawn_blocking wrapping!
    #[tracing::instrument(skip(self))]
    pub async fn moderate_account(
        &self,
        subject: &Did,
        action: Option<ModAction>,
        source: Option<&Did>,
        message: Option<&str>,
        reference: Option<&str>,
    ) -> Result<(), EndpointError> {
        let (tx, rx) = oneshot::channel();
        let now = SystemTime::now();
        let event = ModerateEvent {
            at: now,
            subject: subject.clone(),
            action,
            source: source.cloned(),
            message: message.map(str::to_string),
            reference: reference.map(str::to_string),
        };

        self.repo_sender
            .try_send(
                subject,
                RepoMessage::Moderate {
                    event: Box::new(event),
                    reply: Some(tx),
                },
            )
            .map_err(|err| match err {
                RepoSendError::Backpressure(_) | RepoSendError::EvictionStuck(_) => {
                    error!(%err, "repo send errored, possibly transient");
                    EndpointError {
                        status: StatusCode::SERVICE_UNAVAILABLE,
                        error: "Busy",
                        message: Some("failed to send moderate event"),
                    }
                }
                RepoSendError::Draining(_) => EndpointError {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    error: "ShuttingDown",
                    message: Some("hubble-sync shutting down"),
                },
            })?;

        let outcome = rx.await.map_err(|_| {
            error!("no reply from moderate send (reply chan dropped?)");
            EndpointError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                error: "InternalError",
                message: Some("no reply to moderate event send"),
            }
        })?;

        if let ModerateOutcome::Failed { error, message } = outcome {
            error!(%error, ?message, "failed to apply moderation");
            return Err(EndpointError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                error: "InternalError",
                message: Some("failed to apply moderation"),
            });
        }

        Ok(())
    }

    /// reverse-chronological view of local moderation actions
    #[tracing::instrument(skip(self))]
    pub fn mod_log(
        &self,
        limit: NonZeroUsize,
        cursor: Option<&str>,
    ) -> Result<(Vec<ModerateEvent>, Option<String>), EndpointError> {
        let after = cursor
            .map(|s| [s.as_bytes(), &[0x00]].concat())
            .unwrap_or(vec![]);

        let actions = ModerateEvent::scan(&self.storage, &after)
            .take(limit.into())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                error!(%err, "mod_log mod scan failed");
                EndpointError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    error: "InternalError",
                    message: Some("failed to scan mod log"),
                }
            })?;

        let next_cursor = None; // TODO
        // it's annoying because this feels like we're doing the cursor at the
        // wrong level or in the wrong place.
        // it should proably be opaque bytes from the storage layer, encoded to
        // string by the endpoint or something.

        // TODO: the actions vec should be converted to something that deals
        // with the "None" actions (removing a previous moderation action)

        Ok((actions, next_cursor))
    }

    /// reverse-chronological view of repo #account events
    #[tracing::instrument(skip(self))]
    pub fn upstream_account_log(
        &self,
        did: &Did,
        limit: NonZeroUsize,
        cursor: Option<&str>,
    ) -> Result<(Vec<AccountStatusEvent>, Option<String>), EndpointError> {
        let after = cursor
            .map(|s| [s.as_bytes(), &[0x00]].concat())
            .unwrap_or(vec![]);

        let statuses = AccountStatusEvent::scan(did, &after, &self.storage, &self.hosts)
            .take(limit.into())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                error!(%err, "mod_log mod scan failed");
                EndpointError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    error: "InternalError",
                    message: Some("failed to scan mod log"),
                }
            })?;

        let next_cursor = None; // TODO
        // it's annoying because this feels like we're doing the cursor at the
        // wrong level or in the wrong place.
        // it should proably be opaque bytes from the storage layer, encoded to
        // string by the endpoint or something.

        // TODO: the statuses vec should be converted to something that deals
        // with the "None" statuses (removing a previous moderation action)

        Ok((statuses, next_cursor))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GetRepoStatusResponse {
    #[serde(with = "crate::did::did_atproto")]
    pub did: Did,
    #[serde(with = "crate::tid::tid_atproto")]
    pub rev: Tid,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListReposResponse {
    /// list of repos
    ///
    /// *almost* like com.atproto.sync.listRepos repos, but without `head`
    pub repos: Vec<GetRepoStatusResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// "endpoint" calls return this error type
///
/// these calls are typically used to implement xrpc api endpoints on an http
/// server, hence the xrpc-error-looking shape.
#[derive(Debug, thiserror::Error)]
#[error("EndpointError: status={status} error={error} message={message:?}")]
pub struct EndpointError {
    pub status: StatusCode,
    pub error: &'static str,
    pub message: Option<&'static str>,
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::storage::engine::mem::MemEngine;
    use crate::storage::repo::{
        AccountSyncState, DesyncReason, Desynchronized, RepoIdentity, RepoInfo,
    };
    use crate::{DaslCid, Repo, RepoMessage, RepoSendError, SigningKey, StorageBatch};

    fn hosts() -> Arc<HostRegistry> {
        HostRegistry::new_default()
    }

    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    fn test_cid() -> DaslCid {
        let mut bytes = [0u8; 36];
        bytes[0] = 0x01; // cidv1
        bytes[1] = 0x71; // dag-cbor
        bytes[2] = 0x12; // sha256
        bytes[3] = 0x20; // 32-byte digest
        DaslCid::from_bytes_raw(&bytes).expect("test cid")
    }

    /// seed a repo's `ri|` info (always) and, if `rev` is `Some`, its `si|` sync
    /// state — i.e. a complete repo that `list_synced` will surface.
    ///
    /// generic so it can seed through either a raw engine (SyncEndpoints
    /// tests) or the PrefixedEngine view a SyncHandle reads from.
    fn seed_repo<S: StorageEngine>(
        eng: &S,
        hosts: &HostRegistry,
        did: &Did,
        account_status: AccountStatus,
        sync_status: SyncStatus,
        rev: Option<u64>,
    ) {
        let now = SystemTime::now();
        let info = RepoInfo {
            upstream_status: account_status,
            moderation: None,
            sync_status,
            identity: RepoIdentity {
                pds_host: hosts.get("pds.example.com").expect("interned"),
                signing_key: SigningKey::raw(vec![0u8; 32]),
                supposed_handle: Some("alice.example".to_string()),
                resolved_at: now,
            },
            first_seen_at: now,
            resyncs: 0,
            pds_changes: 0,
            handle_changes: 0,
            last_resync: None,
            first_resync_needs_permit: None,
        };
        let mut b = eng.batch();
        info.store(did, &mut b);
        if let Some(rev) = rev {
            AccountSyncState {
                prev: test_cid(),
                rev: rev.try_into().expect("valid tid"),
                commits: 0,
                other_events: 0,
                records_count_delta: 0,
            }
            .store(did, &mut b);
        }
        b.commit().unwrap();
    }

    fn n(x: usize) -> NonZeroUsize {
        NonZeroUsize::new(x).unwrap()
    }

    struct FakeSender;
    impl FakeSender {
        fn new() -> Arc<Self> {
            Arc::new(Self)
        }
    }
    impl RepoSender for FakeSender {
        fn try_send(&self, _: &Did, _: RepoMessage) -> Result<(), RepoSendError> {
            Ok(())
        }
    }

    // --- get_repo_status ---

    #[test]
    fn get_repo_status_active_synchronized() {
        let (eng, hs, fs) = (MemEngine::new(), hosts(), FakeSender::new());
        let did = plc_did("alice");
        seed_repo(
            &eng,
            &hs,
            &did,
            AccountStatus::Active,
            SyncStatus::Synchronized,
            Some(100),
        );
        let ep = SyncEndpoints::new(eng, hs, fs);
        let resp = ep.get_repo_status(&did).expect("ok");
        assert_eq!(resp.did, did);
        assert!(resp.active);
        assert_eq!(resp.status, None);
        assert_eq!(resp.rev, 100u64.try_into().unwrap());
    }

    #[test]
    fn get_repo_status_desynchronized() {
        let (eng, hs, fs) = (MemEngine::new(), hosts(), FakeSender::new());
        let did = plc_did("desync");
        let sync = SyncStatus::Desynchronized(Desynchronized::new(
            DesyncReason::FirstSeen,
            SystemTime::now(),
            0,
            None,
        ));
        seed_repo(&eng, &hs, &did, AccountStatus::Active, sync, Some(1));
        let ep = SyncEndpoints::new(eng, hs, fs);
        let resp = ep.get_repo_status(&did).expect("ok");
        assert!(resp.active);
        assert_eq!(resp.status, Some("desynchronized"));
    }

    #[test]
    fn get_repo_status_takendown_is_inactive() {
        let (eng, hs, fs) = (MemEngine::new(), hosts(), FakeSender::new());
        let did = plc_did("takendown");
        seed_repo(
            &eng,
            &hs,
            &did,
            AccountStatus::Takendown,
            SyncStatus::Synchronized,
            Some(1),
        );
        let ep = SyncEndpoints::new(eng, hs, fs);
        let resp = ep.get_repo_status(&did).expect("ok");
        assert!(!resp.active);
        assert_eq!(resp.status, Some("takendown"));
    }

    #[test]
    fn get_repo_status_not_found() {
        let ep = SyncEndpoints::new(MemEngine::new(), hosts(), FakeSender::new());
        let err = ep.get_repo_status(&plc_did("ghost")).expect_err("missing");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.error, "RepoNotFound");
    }

    #[test]
    fn get_repo_status_without_rev_is_not_found() {
        // repo info exists but no sync state yet (no rev).
        let (eng, hs, fs) = (MemEngine::new(), hosts(), FakeSender::new());
        let did = plc_did("norev");
        seed_repo(
            &eng,
            &hs,
            &did,
            AccountStatus::Active,
            SyncStatus::Synchronized,
            None,
        );
        let ep = SyncEndpoints::new(eng, hs, fs);
        let err = ep.get_repo_status(&did).expect_err("no rev");
        assert_eq!(err.error, "RepoNotFound");
        assert_eq!(err.message, Some("not yet synchronized (no rev)"));
    }

    // --- list_repos ---

    #[test]
    fn list_repos_returns_seeded_in_did_order() {
        let (eng, hs, fs) = (MemEngine::new(), hosts(), FakeSender::new());
        for label in ["c", "a", "b"] {
            seed_repo(
                &eng,
                &hs,
                &plc_did(label),
                AccountStatus::Active,
                SyncStatus::Synchronized,
                Some(1),
            );
        }
        let ep = SyncEndpoints::new(eng, hs, fs);
        let resp = ep.list_repos(n(10), None).expect("ok");
        let dids: Vec<_> = resp
            .repos
            .iter()
            .map(|r| r.did.as_str().to_owned())
            .collect();
        assert_eq!(
            dids,
            vec![
                plc_did("a").as_str(),
                plc_did("b").as_str(),
                plc_did("c").as_str()
            ]
        );
        assert_eq!(resp.cursor, None, "no cursor when under the limit");
    }

    #[test]
    fn list_repos_paginates_with_cursor() {
        let (eng, hs, fs) = (MemEngine::new(), hosts(), FakeSender::new());
        for label in ["a", "b", "c"] {
            seed_repo(
                &eng,
                &hs,
                &plc_did(label),
                AccountStatus::Active,
                SyncStatus::Synchronized,
                Some(1),
            );
        }
        let ep = SyncEndpoints::new(eng, hs, fs);
        let page1 = ep.list_repos(n(2), None).expect("ok");
        assert_eq!(page1.repos.len(), 2);
        let cursor = page1.cursor.expect("cursor at the limit");
        assert_eq!(cursor, plc_did("b").as_str());

        let page2 = ep.list_repos(n(2), Some(&cursor)).expect("ok");
        let dids: Vec<_> = page2
            .repos
            .iter()
            .map(|r| r.did.as_str().to_owned())
            .collect();
        assert_eq!(dids, vec![plc_did("c").as_str()]);
        assert_eq!(page2.cursor, None);
    }

    #[test]
    fn list_repos_invalid_cursor_is_bad_request() {
        let ep = SyncEndpoints::new(MemEngine::new(), hosts(), FakeSender::new());
        let err = ep
            .list_repos(n(10), Some("not-a-did"))
            .expect_err("bad cursor");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.error, "InvalidRequest");
    }

    // --- Repo::list_synced hardened error path ---

    #[test]
    fn list_synced_errors_on_sync_state_without_info() {
        // si| present, ri| missing: a corruption we surface, not silently skip.
        let (eng, hs) = (MemEngine::new(), hosts());
        let did = plc_did("orphan");
        let mut b = eng.batch();
        AccountSyncState {
            prev: test_cid(),
            rev: 1u64.try_into().unwrap(),
            commits: 0,
            other_events: 0,
            records_count_delta: 0,
        }
        .store(&did, &mut b);
        b.commit().unwrap();

        let err = Repo::list_synced(&eng, &hs, None, n(10)).expect_err("integrity");
        assert!(matches!(err, LoadError::Integrity(_)));
    }

    // --- SyncHandle::get (slot-aware read) ---

    #[test]
    fn get_returns_none_for_missing_repo() {
        let handle =
            SyncHandle::<_, (), ()>::new(MemEngine::new_prefixed(), hosts(), FakeSender::new());
        assert!(handle.get(&plc_did("ghost")).unwrap().is_none());
    }

    #[test]
    fn get_returns_repo_view_with_context() {
        let (eng, hs, fs) = (MemEngine::new_prefixed(), hosts(), FakeSender::new());
        let did = plc_did("alice");
        seed_repo(
            &eng,
            &hs,
            &did,
            AccountStatus::Active,
            SyncStatus::Synchronized,
            Some(42),
        );
        let handle = SyncHandle::<_, (), ()>::new(eng, hs, fs);
        let view = handle.get(&did).unwrap().expect("present");
        assert_eq!(view.context().did(), &did);
        assert_eq!(view.context().rev(), Some(42u64.try_into().unwrap()));
    }

    #[test]
    fn get_with_unit_slots_has_no_slot_data() {
        // unit slots are never present, so the view carries no app-owned data.
        let (eng, hs, fs) = (MemEngine::new_prefixed(), hosts(), FakeSender::new());
        let did = plc_did("noslots");
        seed_repo(
            &eng,
            &hs,
            &did,
            AccountStatus::Active,
            SyncStatus::Synchronized,
            Some(1),
        );
        let handle = SyncHandle::<_, (), ()>::new(eng, hs, fs);
        let view = handle.get(&did).unwrap().expect("present");
        assert!(view.commit().is_none());
        assert!(view.info().is_none());
    }

    // --- repo_counts ---

    fn repo_identity(hosts: &HostRegistry, resolved_at: SystemTime) -> RepoIdentity {
        RepoIdentity {
            pds_host: hosts.get("pds.example.com").expect("interned"),
            signing_key: SigningKey::raw(vec![0u8; 32]),
            supposed_handle: Some("alice.example".to_string()),
            resolved_at,
        }
    }

    #[test]
    fn repo_counts_zero_when_empty() {
        let ep = SyncEndpoints::new(MemEngine::new(), hosts(), FakeSender::new());
        let counts = ep.repo_counts_by_state().unwrap();
        assert_eq!(counts.synchronized, 0);
        assert_eq!(counts.desynchronized, 0);
        assert_eq!(counts.non_active, 0);
        assert_eq!(counts.gone, 0);
    }

    #[test]
    fn repo_counts_reports_bucketed_totals() {
        let (eng, hs, fs) = (MemEngine::new(), hosts(), FakeSender::new());
        let n = SystemTime::now();
        let mut b = eng.batch();
        // three repos born (desynchronized); move one to synchronized.
        Repo::create_resolved(
            plc_did("a"),
            repo_identity(&hs, n),
            AccountStatus::Active,
            n,
            &mut b,
        );
        Repo::create_resolved(
            plc_did("b"),
            repo_identity(&hs, n),
            AccountStatus::Active,
            n,
            &mut b,
        );
        let mut c = Repo::create_resolved(
            plc_did("c"),
            repo_identity(&hs, n),
            AccountStatus::Active,
            n,
            &mut b,
        );
        c.set_sync_status(SyncStatus::Synchronized, &mut b);
        b.commit().unwrap();

        let counts = SyncEndpoints::new(eng, hs, fs)
            .repo_counts_by_state()
            .unwrap();
        assert_eq!(counts.desynchronized, 2);
        assert_eq!(counts.synchronized, 1);
        assert_eq!(counts.gone, 0);
        assert_eq!(counts.non_active, 0);
    }
}
