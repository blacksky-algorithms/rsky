//! re-resolution for previously-seen DIDs when required
//!
//! we extend some grace ("stale" period) to PLC identities tha twe haven't been
//! able to refresh, but eventually we have to call it, and arrive here on next
//! wake.
//!
//! did:webs get unlimited grace -- if we resolved once, they can get stale, but
//! we never hard-block on needing to re-resolve.

use std::sync::Arc;
use std::time::SystemTime;

use tokio::task::spawn_blocking;

use super::ProcessError;
use crate::identity::Resolve;
use crate::storage::repo::{PendingIdentity, Repo};
use crate::{StorageBatch, StorageEngine};

pub(super) struct IdentityRefresh<S: StorageEngine, R: Resolve> {
    pub(super) storage: S,
    pub(super) resolver: Arc<R>,
    pub(super) pending: PendingIdentity,
}

pub(super) enum IdentityRefreshOutcome {
    Snoozed,
    Failed,
    Refreshed,
}

impl<S: StorageEngine, R: Resolve> IdentityRefresh<S, R> {
    pub(super) async fn run(
        self,
        repo: &mut Repo,
        now: SystemTime,
    ) -> Result<IdentityRefreshOutcome, ProcessError<S::Error>> {
        let Self {
            storage,
            resolver,
            mut pending,
        } = self;

        if !pending.is_ready(now) {
            return Ok(IdentityRefreshOutcome::Snoozed);
        }

        let did = repo.did().clone();

        match resolver.resolve(&did, now).await {
            Ok(identity) => {
                let mut repo_inner = repo.clone();
                let storage_inner = storage.clone();

                *repo = spawn_blocking(move || {
                    let mut batch = storage_inner.batch();
                    repo_inner.set_identity(identity, now, &mut batch);
                    pending.delete(&mut batch); // TODO is this actually noop for in-mem pending or are we dropping a tombstone??
                    batch.commit()?;
                    Ok(repo_inner)
                })
                .await
                .expect("storage not to panic")
                .map_err(ProcessError::Storage)?;

                Ok(IdentityRefreshOutcome::Refreshed)
            }
            Err(err) => {
                let mut repo_inner = repo.clone();
                let storage_inner = storage.clone();
                *repo = spawn_blocking(move || {
                    let mut batch = storage_inner.batch();
                    pending.store_failed(&format!("resolve: {err}"), now, &mut batch);
                    // cross-schedule coupling: if the repo is Desync, align
                    // its resync due_at with the new pending next_try_at.
                    repo_inner.delay_resync_until(pending.next_try_at(), &mut batch);
                    batch.commit()?;
                    Ok(repo_inner)
                })
                .await
                .expect("storage not to panic")
                .map_err(ProcessError::Storage)?;
                Ok(IdentityRefreshOutcome::Failed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::time::Duration;

    use super::*;
    use crate::identity::{HubbleSyncResolver, ResolutionError, ResolvedIdentity, SigningKey};
    use crate::storage::engine::mem::MemEngine;
    use crate::storage::repo::{
        AccountStatus, Awoken, DesyncReason, Desynchronized, PendingIdentityQueueEntry,
        RepoIdentity, RepoInfo, SyncStatus,
    };
    use crate::{Did, HostRegistry};

    /// Returns `Ok(identity)` on every call. Lets us pin the success path
    /// of IdentityRefresh without HTTP.
    struct StubOkResolver(RepoIdentity);
    impl Resolve for StubOkResolver {
        fn resolve(
            &self,
            _did: &Did,
            _now: SystemTime,
        ) -> impl Future<Output = ResolvedIdentity> + Send {
            let id = self.0.clone();
            async move { Ok(id) }
        }
    }

    /// Returns the configured error on every call. Lets us drive the
    /// Failed path without HTTP.
    struct StubErrResolver(ResolutionError);
    impl Resolve for StubErrResolver {
        fn resolve(
            &self,
            _did: &Did,
            _now: SystemTime,
        ) -> impl Future<Output = ResolvedIdentity> + Send {
            let err = self.0.clone();
            async move { Err(err) }
        }
    }

    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    fn install_expired_repo(storage: &MemEngine, hosts: &HostRegistry, did: &Did) {
        let pds_host = hosts.get("pds.example.com").expect("interned");
        // 4 days back: past PLC expire (3d).
        let expired_at = SystemTime::now() - Duration::from_secs(4 * 86_400);
        let info = RepoInfo {
            upstream_status: AccountStatus::Active,
            moderation: None,
            sync_status: SyncStatus::Synchronized,
            identity: RepoIdentity {
                pds_host,
                signing_key: SigningKey::raw(vec![0u8; 32]),
                supposed_handle: Some("alice.example".to_string()),
                resolved_at: expired_at,
            },
            first_seen_at: expired_at,
            resyncs: 0,
            pds_changes: 0,
            handle_changes: 0,
            last_resync: None,
            first_resync_needs_permit: None,
        };
        let mut b = storage.batch();
        info.store(did, &mut b);
        b.commit().unwrap();
    }

    /// Build the IdentityRefresh struct's resolver — only constructable shape;
    /// we never actually hit it in the Snoozed path since `is_ready`
    /// short-circuits.
    fn dummy_resolver(hosts: Arc<HostRegistry>) -> Arc<HubbleSyncResolver> {
        Arc::new(HubbleSyncResolver::new(
            "@bad-example.com",
            "https://plc.directory",
            hosts,
        ))
    }

    #[tokio::test]
    async fn refresh_returns_snoozed_when_pending_not_ready() {
        let storage = MemEngine::new();
        let hosts = HostRegistry::new_default();
        let did = plc_did("snooze");
        install_expired_repo(&storage, &hosts, &did);

        // get a real Repo via wake — wake returns Refreshing for an
        // expired identity.
        let awoken = Repo::wake::<_, (), ()>(&storage, &hosts, did.clone(), SystemTime::now())
            .expect("wake ok")
            .0;
        let mut repo = match awoken {
            Awoken::Refreshing { repo, .. } => repo,
            _ => panic!("test precondition: expected Refreshing"),
        };
        let pds_host_before = repo.info().identity.pds_host.clone();

        // construct a not-ready pending: one prior failure puts next_try_at
        // into the future by the PLC backoff schedule.
        let mut pending = PendingIdentity::new(did.clone(), SystemTime::now());
        let mut b = storage.batch();
        pending.store_failed("prior", SystemTime::now(), &mut b);
        b.commit().unwrap();
        assert!(
            !pending.is_ready(SystemTime::now()),
            "test precondition: pending must not be ready"
        );

        let refresh = IdentityRefresh {
            storage: storage.clone(),
            resolver: dummy_resolver(hosts),
            pending,
        };
        let outcome = refresh
            .run(&mut repo, SystemTime::now())
            .await
            .expect("storage ok");
        assert!(matches!(outcome, IdentityRefreshOutcome::Snoozed));

        // identity unchanged: same Arc<Host> instance for the PDS
        assert!(
            Arc::ptr_eq(&repo.info().identity.pds_host, &pds_host_before),
            "Snoozed must not mutate identity"
        );
    }

    #[tokio::test]
    async fn refresh_snoozed_does_not_modify_storage() {
        // Snooze should be inert wrt storage: no batch is opened, no commit.
        // We verify by snapshotting any state we can observe.
        let storage = MemEngine::new();
        let hosts = HostRegistry::new_default();
        let did = plc_did("snooze_noop");
        install_expired_repo(&storage, &hosts, &did);

        let mut pending = PendingIdentity::new(did.clone(), SystemTime::now());
        let mut b = storage.batch();
        pending.store_failed("prior", SystemTime::now(), &mut b);
        b.commit().unwrap();
        let stored_before = PendingIdentity::load(&storage, &did)
            .unwrap()
            .expect("stored");

        let mut repo =
            match Repo::wake::<_, (), ()>(&storage, &hosts, did.clone(), SystemTime::now())
                .expect("wake ok")
                .0
            {
                Awoken::Refreshing { repo, .. } => repo,
                _ => panic!("expected Refreshing"),
            };

        let refresh = IdentityRefresh {
            storage: storage.clone(),
            resolver: dummy_resolver(hosts),
            pending,
        };
        let _ = refresh
            .run(&mut repo, SystemTime::now())
            .await
            .expect("storage ok");

        // pending state on disk unchanged
        let stored_after = PendingIdentity::load(&storage, &did)
            .unwrap()
            .expect("still stored");
        assert_eq!(stored_after.next_try_at(), stored_before.next_try_at());
    }

    // --- success path ---

    /// Same shape as install_expired_repo but lets the caller specify the
    /// sync_status to exercise the cross-schedule coupling.
    fn install_expired_repo_with_status(
        storage: &MemEngine,
        hosts: &HostRegistry,
        did: &Did,
        sync_status: SyncStatus,
    ) {
        let pds_host = hosts.get("pds.example.com").expect("interned");
        let expired_at = SystemTime::now() - Duration::from_secs(4 * 86_400);
        let info = RepoInfo {
            upstream_status: AccountStatus::Active,
            moderation: None,
            sync_status,
            identity: RepoIdentity {
                pds_host,
                signing_key: SigningKey::raw(vec![0u8; 32]),
                supposed_handle: Some("alice.example".to_string()),
                resolved_at: expired_at,
            },
            first_seen_at: expired_at,
            resyncs: 0,
            pds_changes: 0,
            handle_changes: 0,
            last_resync: None,
            first_resync_needs_permit: None,
        };
        let mut b = storage.batch();
        info.store(did, &mut b);
        b.commit().unwrap();
    }

    /// Build a fresh, resolved identity record for a stub-OK resolver to
    /// return — distinguishable from the expired one already installed.
    fn fresh_identity(hosts: &HostRegistry, signing_key: SigningKey) -> RepoIdentity {
        RepoIdentity {
            pds_host: hosts.get("plc.example.com").expect("interned"),
            signing_key,
            supposed_handle: Some("alice.example".to_string()),
            resolved_at: SystemTime::now(),
        }
    }

    #[tokio::test]
    async fn refresh_refreshed_updates_identity_in_memory_and_on_disk() {
        let storage = MemEngine::new();
        let hosts = HostRegistry::new_default();
        let did = plc_did("ok_path");
        install_expired_repo(&storage, &hosts, &did);

        let mut repo =
            match Repo::wake::<_, (), ()>(&storage, &hosts, did.clone(), SystemTime::now())
                .expect("wake ok")
                .0
            {
                Awoken::Refreshing { repo, .. } => repo,
                _ => panic!("precondition: expected Refreshing"),
            };
        let stale_key = repo.info().identity.signing_key.clone();
        let pending = PendingIdentity::new(did.clone(), SystemTime::now());
        assert!(pending.is_ready(SystemTime::now()));

        let new_id = fresh_identity(&hosts, SigningKey::raw(vec![0xAA, 0xBB, 0xCC]));
        let refresh = IdentityRefresh {
            storage: storage.clone(),
            resolver: Arc::new(StubOkResolver(new_id.clone())),
            pending,
        };
        let outcome = refresh
            .run(&mut repo, SystemTime::now())
            .await
            .expect("storage ok");
        assert!(matches!(outcome, IdentityRefreshOutcome::Refreshed));

        // in-memory repo got the new identity
        assert_eq!(repo.info().identity.signing_key, new_id.signing_key);
        assert_ne!(repo.info().identity.signing_key, stale_key);
        // and the same is on disk
        let reg2 = HostRegistry::new_default();
        let loaded = RepoInfo::load(&storage, &reg2, &did)
            .unwrap()
            .expect("ri/ present");
        assert_eq!(loaded.identity.signing_key, new_id.signing_key);
    }

    #[tokio::test]
    async fn refresh_refreshed_clears_both_pi_and_pq_when_pending_was_persisted() {
        // Regression check on PendingIdentity::delete: after a prior failure
        // stored both pi| and pq|, a successful refresh must clear both — not
        // orphan the pq| index entry.
        let storage = MemEngine::new();
        let hosts = HostRegistry::new_default();
        let did = plc_did("ok_after_fail");
        install_expired_repo(&storage, &hosts, &did);

        // simulate a prior failure: writes pi| + pq|
        let mut pending = PendingIdentity::new(did.clone(), SystemTime::now());
        let mut b = storage.batch();
        pending.store_failed("first", SystemTime::now(), &mut b);
        b.commit().unwrap();
        assert!(PendingIdentity::load(&storage, &did).unwrap().is_some());
        assert_eq!(
            PendingIdentityQueueEntry::scan(&storage).count(),
            1,
            "pq| should have one entry after the prior failure"
        );

        // force the pending into the ready window so refresh proceeds
        let bump_now = pending.next_try_at() + Duration::from_secs(1);

        let mut repo = match Repo::wake::<_, (), ()>(&storage, &hosts, did.clone(), bump_now)
            .expect("wake ok")
            .0
        {
            Awoken::Refreshing { repo, .. } => repo,
            _ => panic!("precondition: expected Refreshing"),
        };

        let new_id = fresh_identity(&hosts, SigningKey::raw(vec![0xDE, 0xAD, 0xBE, 0xEF]));
        let refresh = IdentityRefresh {
            storage: storage.clone(),
            resolver: Arc::new(StubOkResolver(new_id)),
            pending,
        };
        let outcome = refresh.run(&mut repo, bump_now).await.expect("storage ok");
        assert!(matches!(outcome, IdentityRefreshOutcome::Refreshed));

        // both pi| and pq| must be gone
        assert!(
            PendingIdentity::load(&storage, &did).unwrap().is_none(),
            "pi| should be cleared on Refreshed"
        );
        assert_eq!(
            PendingIdentityQueueEntry::scan(&storage).count(),
            0,
            "pq| index entry must also be cleared (no orphan)"
        );
    }

    // --- failure path ---

    #[tokio::test]
    async fn refresh_failed_writes_pending_with_bumped_attempts() {
        let storage = MemEngine::new();
        let hosts = HostRegistry::new_default();
        let did = plc_did("fail_path");
        install_expired_repo(&storage, &hosts, &did);

        let mut repo =
            match Repo::wake::<_, (), ()>(&storage, &hosts, did.clone(), SystemTime::now())
                .expect("wake ok")
                .0
            {
                Awoken::Refreshing { repo, .. } => repo,
                _ => panic!("precondition: expected Refreshing"),
            };
        let pending = PendingIdentity::new(did.clone(), SystemTime::now());
        assert!(pending.is_ready(SystemTime::now()));

        let refresh = IdentityRefresh {
            storage: storage.clone(),
            resolver: Arc::new(StubErrResolver(ResolutionError::Network(
                "boom".to_string(),
            ))),
            pending,
        };
        let outcome = refresh
            .run(&mut repo, SystemTime::now())
            .await
            .expect("storage ok");
        assert!(matches!(outcome, IdentityRefreshOutcome::Failed));

        // pi| now persisted with attempts=1, last_error captured
        let loaded = PendingIdentity::load(&storage, &did)
            .unwrap()
            .expect("pi| must be persisted after Failed");
        assert!(
            !loaded.is_ready(SystemTime::now()),
            "next_try_at should be in the future after backoff"
        );
        // pq| index also has the entry
        assert_eq!(
            PendingIdentityQueueEntry::scan(&storage).count(),
            1,
            "pq| index should track the failure for scheduler retry"
        );
    }

    #[tokio::test]
    async fn refresh_failed_aligns_resync_due_at_when_desync() {
        // Cross-schedule coupling: when the repo is already Desync, a failed
        // refresh bumps the resync due_at to match the pending's new
        // next_try_at, so the resync scheduler doesn't dispatch a resync the
        // actor can't service yet.
        let storage = MemEngine::new();
        let hosts = HostRegistry::new_default();
        let did = plc_did("coupling");
        install_expired_repo_with_status(
            &storage,
            &hosts,
            &did,
            SyncStatus::Desynchronized(Desynchronized::new(
                DesyncReason::FirstSeen,
                SystemTime::now(),
                0,
                None,
            )),
        );

        let mut repo =
            match Repo::wake::<_, (), ()>(&storage, &hosts, did.clone(), SystemTime::now())
                .expect("wake ok")
                .0
            {
                Awoken::Refreshing { repo, .. } => repo,
                _ => panic!("precondition: expected Refreshing"),
            };
        let pending = PendingIdentity::new(did.clone(), SystemTime::now());

        let refresh = IdentityRefresh {
            storage: storage.clone(),
            resolver: Arc::new(StubErrResolver(ResolutionError::Network(
                "boom".to_string(),
            ))),
            pending,
        };
        let _ = refresh
            .run(&mut repo, SystemTime::now())
            .await
            .expect("storage ok");

        // Compare on-disk values: both go through the same ms-precision serde,
        // so they're directly comparable. The in-memory repo carries sub-ms
        // precision the pending storage doesn't preserve.
        let loaded_pending = PendingIdentity::load(&storage, &did).unwrap().expect("pi|");
        let reg2 = HostRegistry::new_default();
        let loaded_repo = RepoInfo::load(&storage, &reg2, &did).unwrap().expect("ri|");
        let loaded_due = match loaded_repo.sync_status {
            SyncStatus::Desynchronized(d) => d.due_at(),
            _ => panic!("resync status must remain Desynchronized after Failed refresh"),
        };
        assert_eq!(
            loaded_due,
            loaded_pending.next_try_at(),
            "resync due_at must follow pending next_try_at"
        );
    }

    #[tokio::test]
    async fn refresh_failed_leaves_sync_status_alone_when_synchronized() {
        // The companion to the coupling test: when the repo isn't Desync,
        // delay_resync_until is a no-op — refresh failure shouldn't pull the
        // repo into Desync.
        let storage = MemEngine::new();
        let hosts = HostRegistry::new_default();
        let did = plc_did("synchronized");
        // already Synchronized
        install_expired_repo(&storage, &hosts, &did);

        let mut repo =
            match Repo::wake::<_, (), ()>(&storage, &hosts, did.clone(), SystemTime::now())
                .expect("wake ok")
                .0
            {
                Awoken::Refreshing { repo, .. } => repo,
                _ => panic!("precondition: expected Refreshing"),
            };
        let pending = PendingIdentity::new(did.clone(), SystemTime::now());

        let refresh = IdentityRefresh {
            storage: storage.clone(),
            resolver: Arc::new(StubErrResolver(ResolutionError::Network(
                "boom".to_string(),
            ))),
            pending,
        };
        let _ = refresh
            .run(&mut repo, SystemTime::now())
            .await
            .expect("storage ok");

        assert!(
            matches!(repo.info().sync_status, SyncStatus::Synchronized),
            "non-Desync repo must stay Synchronized on Failed refresh"
        );
    }
}
