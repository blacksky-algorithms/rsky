//! Whether an account's state on this server agrees with what it has
//! published, with nothing owed to any worker.
//!
//! The report reads every journal and compares: the store's root, the
//! account database's root, and the last published commit must be the same
//! commit; the account's status and handle must match the last events that
//! announced them; and no publication intent, blob work, quarantine,
//! repair, or deletion may be outstanding. For a deleted account the test
//! is that the deletion itself is complete; whether its objects were purged
//! is reported separately and never blocks logical convergence.

use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::AccountManager;
use crate::actor_store::blob::BlobWorkState;
use crate::actor_store::blobstore::unavailable;
use crate::actor_store::ActorStore;
use crate::lifecycle::LifecycleStore;
use crate::repair::RepairStore;
use crate::sequencer::events::{AccountEvt, CommitEvt, IdentityEvt};
use crate::SharedSequencer;
use anyhow::Result;
use rsky_common::cbor_to_struct;
use rsky_lexicon::com::atproto::sync::AccountStatus as LexiconAccountStatus;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;

/// Blob work older than this that is still not terminal is flagged.
pub const OVERDUE_AFTER: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootRef {
    pub cid: String,
    pub rev: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountConsistency {
    pub current_status: Option<String>,
    pub last_event_status: Option<String>,
    pub current_handle: Option<String>,
    pub last_event_handle: Option<String>,
    pub consistent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleState {
    pub tombstone_open: bool,
    pub logically_deleted: bool,
    pub purge_obligation_open: bool,
    pub actor_directory_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Convergence {
    pub did: String,
    pub converged: bool,
    /// Every way the account falls short, empty when converged.
    pub reasons: Vec<String>,
    /// The account rows are gone and the last account event is a deletion.
    pub deleted: bool,
    pub store_root: Option<RootRef>,
    pub account_root: Option<RootRef>,
    pub last_published: Option<RootRef>,
    pub pending_intents: usize,
    pub account: AccountConsistency,
    pub blob_work: BTreeMap<String, usize>,
    pub blob_work_nonterminal: usize,
    pub blob_work_overdue: usize,
    pub open_quarantines: Vec<i64>,
    pub pending_repairs: Vec<String>,
    pub lifecycle: LifecycleState,
}

fn status_name(status: &AccountStatus) -> String {
    format!("{status:?}").to_ascii_lowercase()
}

fn lexicon_status_name(status: &Option<LexiconAccountStatus>) -> String {
    match status {
        None => "active".to_owned(),
        Some(status) => serde_json::to_value(status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{status:?}").to_ascii_lowercase()),
    }
}

fn is_overdue(created_at: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|created| {
            let age = chrono::Utc::now().signed_duration_since(created);
            age > chrono::Duration::from_std(OVERDUE_AFTER).expect("in range")
        })
        .unwrap_or(false)
}

pub struct ConvergenceContext<'a> {
    pub actor_store: &'a ActorStore,
    pub account_manager: &'a AccountManager,
    pub sequencer: &'a SharedSequencer,
    pub lifecycle: &'a LifecycleStore,
    pub repairs: &'a RepairStore,
}

pub async fn convergence(ctx: &ConvergenceContext<'_>, did: &str) -> Result<Convergence> {
    let mut reasons = Vec::new();

    // what the sequencer last announced for the account
    let rows = ctx
        .sequencer
        .sequencer
        .read()
        .await
        .rows_for_did_after(did, 0)
        .await?;
    let live = rows.iter().filter(|row| row.invalidated.unwrap_or(0) == 0);
    let mut last_published = None;
    let mut last_event_status = None;
    let mut last_event_handle = None;
    for row in live {
        match row.event_type.as_str() {
            "append" => {
                if let Ok(evt) = cbor_to_struct::<CommitEvt>(row.event.clone()) {
                    last_published = Some(RootRef {
                        cid: evt.commit.to_string(),
                        rev: evt.rev,
                    });
                }
            }
            "account" => {
                if let Ok(evt) = cbor_to_struct::<AccountEvt>(row.event.clone()) {
                    last_event_status = Some(lexicon_status_name(&evt.status));
                }
            }
            "identity" => {
                if let Ok(evt) = cbor_to_struct::<IdentityEvt>(row.event.clone()) {
                    last_event_handle = evt.handle;
                }
            }
            _ => {}
        }
    }

    // the account database and the store
    let account = ctx
        .account_manager
        .get_account(
            did,
            Some(
                crate::account_manager::helpers::account::AvailabilityFlags {
                    include_deactivated: Some(true),
                    include_taken_down: Some(true),
                },
            ),
        )
        .await?;
    let account_root = ctx
        .account_manager
        .get_repo_root(did)
        .await?
        .map(|(cid, rev)| RootRef { cid, rev });
    let current_status = match &account {
        Some(_) => Some(status_name(
            &ctx.account_manager.get_account_status(did).await?,
        )),
        None => None,
    };
    let current_handle = account.as_ref().and_then(|account| account.handle.clone());
    let deleted = account.is_none() && last_event_status.as_deref() == Some("deleted");

    let directory_present = ctx.actor_store.exists(did).await?;
    let (store_root, pending_intents, blob_work, nonterminal, overdue) = if directory_present {
        let reader = ctx.actor_store.read(did.to_owned(), unavailable()).await?;
        let root = reader
            .storage
            .read()
            .await
            .get_root_detailed()
            .await
            .ok()
            .map(|root| RootRef {
                cid: root.cid.to_string(),
                rev: root.rev,
            });
        let journaled = reader
            .record
            .db
            .run(|conn| {
                let count: i64 = conn.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' \
                     AND name IN ('publish_intent', 'blob_work')",
                    [],
                    |row| row.get(0),
                )?;
                Ok(count == 2)
            })
            .await?;
        if journaled {
            let pending = reader.pending_intents().await?.len();
            let work = reader.blob.blob_work().await?;
            let mut by_state: BTreeMap<String, usize> = BTreeMap::new();
            for state in BlobWorkState::ALL {
                by_state.insert(state.as_str().to_owned(), 0);
            }
            let mut nonterminal = 0;
            let mut overdue = 0;
            for row in &work {
                *by_state.entry(row.state.as_str().to_owned()).or_default() += 1;
                if !row.state.is_terminal() {
                    nonterminal += 1;
                    if is_overdue(&row.created_at) {
                        overdue += 1;
                    }
                }
            }
            (root, pending, by_state, nonterminal, overdue)
        } else {
            (root, 0, BTreeMap::new(), 0, 0)
        }
    } else {
        (None, 0, BTreeMap::new(), 0, 0)
    };

    let tombstone = ctx.lifecycle.tombstone_of(did).await?;
    let lifecycle = LifecycleState {
        tombstone_open: tombstone
            .as_ref()
            .is_some_and(|t| t.logically_deleted_at.is_none()),
        logically_deleted: tombstone
            .as_ref()
            .is_some_and(|t| t.logically_deleted_at.is_some()),
        purge_obligation_open: ctx.lifecycle.purge_obligation_of(did).await?.is_some(),
        actor_directory_present: directory_present,
    };
    let open_quarantines = ctx.repairs.open_quarantines_for(did).await?;
    let pending_repairs = ctx.repairs.pending_for(did).await?;

    if pending_intents > 0 {
        reasons.push(format!(
            "{pending_intents} publication intents are undelivered"
        ));
    }
    if !open_quarantines.is_empty() {
        reasons.push(format!("quarantines are open: {open_quarantines:?}"));
    }
    if !pending_repairs.is_empty() {
        reasons.push(format!("repairs are pending: {pending_repairs:?}"));
    }
    if lifecycle.tombstone_open {
        reasons.push("the account's deletion is in progress".to_owned());
    }
    let account_consistency;
    if deleted {
        account_consistency = AccountConsistency {
            current_status: None,
            last_event_status: last_event_status.clone(),
            current_handle: None,
            last_event_handle: last_event_handle.clone(),
            consistent: true,
        };
        if !lifecycle.logically_deleted {
            reasons.push("the deletion was not journaled here".to_owned());
        }
        if directory_present {
            reasons.push("the actor directory still exists".to_owned());
        }
        if account_root.is_some() {
            reasons.push("the account database still records a root".to_owned());
        }
    } else {
        let consistent = current_status == last_event_status
            && (last_event_handle.is_none() || current_handle == last_event_handle);
        if !consistent {
            reasons.push("the account's status or handle differs from its last events".to_owned());
        }
        account_consistency = AccountConsistency {
            current_status,
            last_event_status,
            current_handle,
            last_event_handle,
            consistent,
        };
        if store_root.is_none() || store_root != account_root || store_root != last_published {
            reasons.push("the store, account, and published roots differ".to_owned());
        }
        if nonterminal > 0 {
            reasons.push(format!("{nonterminal} blob work rows are not terminal"));
        }
    }

    Ok(Convergence {
        did: did.to_owned(),
        converged: reasons.is_empty(),
        reasons,
        deleted,
        store_root,
        account_root,
        last_published,
        pending_intents,
        account: account_consistency,
        blob_work,
        blob_work_nonterminal: nonterminal,
        blob_work_overdue: overdue,
        open_quarantines,
        pending_repairs,
        lifecycle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::CreateAccountOpts;
    use crate::actor_store::blobstore::MemoryBlobStore;
    use crate::background::BackgroundQueue;
    use crate::config::ActorStoreConfig;
    use crate::crawlers::Crawlers;
    use crate::publication::publish_pending;
    use crate::sequencer::Sequencer;
    use lexicon_cid::Cid;
    use rsky_repo::types::{PreparedCreateOrUpdate, PreparedWrite, WriteOpAction};
    use secp256k1::{Keypair, Secp256k1, SecretKey};
    use std::str::FromStr;
    use std::sync::Arc;

    const DID: &str = "did:plc:converge";

    struct World {
        _dir: tempfile::TempDir,
        actor_store: ActorStore,
        account_manager: AccountManager,
        sequencer: SharedSequencer,
        lifecycle: LifecycleStore,
        repairs: RepairStore,
        blobstore: Arc<MemoryBlobStore>,
    }

    impl World {
        fn ctx(&self) -> ConvergenceContext<'_> {
            ConvergenceContext {
                actor_store: &self.actor_store,
                account_manager: &self.account_manager,
                sequencer: &self.sequencer,
                lifecycle: &self.lifecycle,
                repairs: &self.repairs,
            }
        }

        async fn report(&self) -> Convergence {
            convergence(&self.ctx(), DID).await.unwrap()
        }
    }

    fn post(rkey: &str) -> PreparedWrite {
        let record: rsky_repo::types::RepoRecord = serde_json::from_value(serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": "converge",
            "createdAt": "2023-01-01T00:00:00.000Z",
        }))
        .unwrap();
        let cid = rsky_common::ipld::cid_for_cbor(&record).unwrap();
        PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: format!("at://{DID}/app.bsky.feed.post/{rkey}"),
            cid,
            swap_cid: None,
            record,
            blobs: vec![],
        })
    }

    async fn world() -> World {
        crate::account_manager::tests::init_env();
        let dir = tempfile::tempdir().unwrap();
        let lifecycle = LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
        let actor_store = ActorStore::new(
            &ActorStoreConfig {
                directory: dir.path().join("actors").to_str().unwrap().to_owned(),
                cache_size: 10,
            },
            BackgroundQueue::default(),
            lifecycle.clone(),
        );
        let account_manager = AccountManager::new(
            crate::account_manager::db::get_migrated_db(dir.path().join("account.sqlite"))
                .await
                .unwrap(),
        );
        let sequencer = SharedSequencer {
            sequencer: tokio::sync::RwLock::new(Sequencer::new(
                crate::sequencer::db::get_migrated_db(dir.path().join("sequencer.sqlite"))
                    .await
                    .unwrap(),
                Crawlers::new("pds.test".to_owned(), vec![]),
                None,
            )),
        };
        let repairs = RepairStore::open(dir.path().join("rsky/repair.sqlite"))
            .await
            .unwrap();
        World {
            _dir: dir,
            actor_store,
            account_manager,
            sequencer,
            lifecycle,
            repairs,
            blobstore: Arc::new(MemoryBlobStore::default()),
        }
    }

    /// Creates the account the way the server does: store, first commit,
    /// account rows, identity and account events, then the commit batch.
    async fn create_account(world: &World) {
        let keypair = Keypair::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[13u8; 32]).unwrap(),
        );
        world.actor_store.create(DID, &keypair).await.unwrap();
        let txn = world
            .actor_store
            .transact(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        let init = txn.create_repo(vec![], true).await.unwrap();
        drop(txn);
        world
            .account_manager
            .create_account(CreateAccountOpts {
                did: DID.to_owned(),
                handle: "converge.test".to_owned(),
                email: Some("converge@example.com".to_owned()),
                password: Some("password123".to_owned()),
                repo_cid: init.commit_data.cid,
                repo_rev: init.commit_data.rev.clone(),
                invite_code: None,
                deactivated: None,
            })
            .await
            .unwrap();
        {
            let mut lock = world.sequencer.sequencer.write().await;
            lock.sequence_identity_evt(DID.to_owned(), Some("converge.test".to_owned()))
                .await
                .unwrap();
            lock.sequence_account_evt(DID.to_owned(), AccountStatus::Active)
                .await
                .unwrap();
        }
        publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_freshly_written_account_converges_and_every_shortfall_is_named() {
        let world = world().await;
        let unknown = world.report().await;
        assert!(!unknown.converged);
        assert!(unknown.reasons.iter().any(|r| r.contains("roots differ")));
        assert!(!unknown.deleted);

        create_account(&world).await;
        let fresh = world.report().await;
        assert!(fresh.converged, "{fresh:?}");
        assert_eq!(fresh.store_root, fresh.account_root);
        assert_eq!(fresh.store_root, fresh.last_published);
        assert!(fresh.account.consistent);
        assert_eq!(fresh.account.current_status.as_deref(), Some("active"));
        assert_eq!(
            fresh.account.last_event_handle.as_deref(),
            Some("converge.test")
        );
        assert_eq!(fresh.blob_work["gc-deferred"], 0);
        let json = serde_json::to_value(&fresh).unwrap();
        assert_eq!(json["converged"], true);
        assert_eq!(json["blobWorkOverdue"], 0);

        // a write that was committed but not yet published or recorded
        let mut txn = world
            .actor_store
            .transact(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        txn.process_writes(vec![post("3lfixtureaa2a")], None)
            .await
            .unwrap();
        drop(txn);
        let behind = world.report().await;
        assert!(!behind.converged);
        assert_eq!(behind.pending_intents, 1);
        assert!(behind.reasons.iter().any(|r| r.contains("undelivered")));
        assert!(behind.reasons.iter().any(|r| r.contains("roots differ")));

        // publishing and recording the root restores convergence
        publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        let root = world
            .actor_store
            .read(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap()
            .storage
            .read()
            .await
            .get_root_detailed()
            .await
            .unwrap();
        world
            .account_manager
            .update_repo_root(DID.to_owned(), root.cid, root.rev)
            .await
            .unwrap();
        assert!(world.report().await.converged);

        // a status change the sequencer never announced
        world
            .account_manager
            .deactivate_account(DID, None)
            .await
            .unwrap();
        let stale = world.report().await;
        assert!(!stale.converged);
        assert!(!stale.account.consistent);
        assert_eq!(stale.account.current_status.as_deref(), Some("deactivated"));
        world
            .sequencer
            .sequencer
            .write()
            .await
            .sequence_account_evt(DID.to_owned(), AccountStatus::Deactivated)
            .await
            .unwrap();
        assert!(world.report().await.converged);

        // open quarantines, pending repairs, and outstanding blob work
        world
            .repairs
            .create(
                "r1",
                DID,
                &crate::repair::RepairKind::EmptyCommit {
                    boundary: "3zzzzzzzzzzzz".to_owned(),
                },
            )
            .await
            .unwrap();
        world
            .repairs
            .open_quarantine(1, DID, "bad-event", &["r1".to_owned()])
            .await
            .unwrap();
        world
            .actor_store
            .read(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap()
            .record
            .db
            .run(|conn| {
                conn.execute(
                    "INSERT INTO blob_work (kind, key, cid, state, \"createdAt\", \"updatedAt\") \
                     VALUES ('permanent', 'k', 'k', 'delete-pending', '2020-01-01T00:00:00.000Z', '2020-01-01T00:00:00.000Z')",
                    [],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let owed = world.report().await;
        assert!(!owed.converged);
        assert_eq!(owed.open_quarantines, [1]);
        assert_eq!(owed.pending_repairs, ["r1"]);
        assert_eq!(owed.blob_work_nonterminal, 1);
        assert_eq!(owed.blob_work_overdue, 1);
        assert_eq!(owed.blob_work["delete-pending"], 1);
        assert_eq!(owed.reasons.len(), 3);
    }

    #[tokio::test]
    async fn a_deleted_account_converges_once_its_deletion_is_journaled() {
        let world = world().await;
        create_account(&world).await;
        world.lifecycle.tombstone(DID).await.unwrap();
        let deleting = world.report().await;
        assert!(!deleting.converged);
        assert!(deleting.lifecycle.tombstone_open);
        assert!(deleting.reasons.iter().any(|r| r.contains("in progress")));

        let ctx = crate::lifecycle::DeletionContext {
            lifecycle: &world.lifecycle,
            account_manager: &world.account_manager,
            sequencer: &world.sequencer,
            actor_store: &world.actor_store,
            blobstore: None,
        };
        crate::lifecycle::delete_account(&ctx, DID, None)
            .await
            .unwrap();
        let deleted = world.report().await;
        assert!(deleted.converged, "{deleted:?}");
        assert!(deleted.deleted);
        assert!(deleted.lifecycle.logically_deleted);
        assert!(deleted.lifecycle.purge_obligation_open);
        assert!(!deleted.lifecycle.actor_directory_present);
        assert_eq!(
            deleted.account.last_event_status.as_deref(),
            Some("deleted")
        );

        // a deletion another implementation performed leaves no journal
        // here, so it is reported rather than assumed complete
        let other = "did:plc:elsewhere";
        world
            .sequencer
            .sequencer
            .write()
            .await
            .sequence_account_evt(other.to_owned(), AccountStatus::Deleted)
            .await
            .unwrap();
        world
            .account_manager
            .update_repo_root(
                other.to_owned(),
                Cid::from_str("bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4")
                    .unwrap(),
                "3lfixtureaa2a".to_owned(),
            )
            .await
            .unwrap();
        let keypair = Keypair::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[14u8; 32]).unwrap(),
        );
        world.actor_store.create(other, &keypair).await.unwrap();
        let foreign = convergence(&world.ctx(), other).await.unwrap();
        assert!(!foreign.converged);
        assert!(foreign.deleted);
        assert!(foreign.reasons.iter().any(|r| r.contains("not journaled")));
        assert!(foreign.reasons.iter().any(|r| r.contains("still exists")));
        assert!(foreign
            .reasons
            .iter()
            .any(|r| r.contains("still records a root")));
    }

    #[test]
    fn names_and_ages() {
        assert_eq!(status_name(&AccountStatus::Takendown), "takendown");
        assert_eq!(lexicon_status_name(&None), "active");
        assert_eq!(
            lexicon_status_name(&Some(LexiconAccountStatus::Deactivated)),
            "deactivated"
        );
        assert!(!is_overdue("not a date"));
        assert!(!is_overdue(&rsky_common::now()));
        assert!(is_overdue("2020-01-01T00:00:00.000Z"));
    }
}
