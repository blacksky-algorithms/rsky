//! How far an actor's publication history on this server reaches, and
//! whether it is provably whole.
//!
//! A downstream index that reconciles against this server needs a boundary
//! it can trust: every event for the actor at or below it is either present
//! here or known to have been published here first. That is only provable
//! for an actor whose whole hosting lifetime was this server and whose
//! history starts with a batch from which nothing was pruned, so the
//! frontier reports both and fails closed on anything it cannot establish.

use crate::actor_store::ActorStore;
use crate::lifecycle::LifecycleStore;
use crate::plc;
use crate::sequencer::events::CommitEvt;
use crate::SharedSequencer;
use anyhow::Result;
use rsky_common::cbor_to_struct;
use rsky_lexicon::com::atproto::sync::AccountStatus as LexiconAccountStatus;
use rsky_repo::repo::Repo;
use serde::Serialize;

/// Whether every host the DID's identity history names is this server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Lifetime {
    HostedHereOnly,
    Mixed,
    Unknown,
}

/// The earliest surviving row group of the actor's publication history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GenesisKind {
    /// The creation batch: identity, account, the first commit, sync.
    Creation,
    /// An activation batch following a deletion event, accepted only when
    /// the re-created repository's first commit exceeds the highest
    /// revision published before the deletion.
    PostDeletion,
    /// An activation batch with nothing before it: the account was migrated
    /// in, and its earlier history was published elsewhere.
    Activation,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationFrontier {
    pub did: String,
    /// The highest revision of any commit event published for the actor.
    pub publication_max_rev: Option<String>,
    /// The highest revision this server ever served or accepted by import.
    pub exposed_max_rev: Option<String>,
    pub current_commit_cid: Option<String>,
    /// The revision inside the signed commit block.
    pub signed_commit_rev: Option<String>,
    /// The revision the store records for the root, which an import may
    /// have rewritten without re-signing.
    pub repo_root_rev: Option<String>,
    pub restore_event_count: i64,
    pub lifetime: Lifetime,
    pub genesis_kind: GenesisKind,
    pub complete: bool,
    /// Why the history is not complete, when it is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Row {
    Identity,
    Account(Option<LexiconAccountStatus>),
    Append { rev: String, since: Option<String> },
    Sync,
    Other,
}

fn classify(row: crate::models::models::RepoSeq) -> Row {
    match row.event_type.as_str() {
        "identity" => Row::Identity,
        "sync" => Row::Sync,
        "account" => cbor_to_struct::<crate::sequencer::events::AccountEvt>(row.event)
            .map(|evt| Row::Account(evt.status))
            .unwrap_or(Row::Other),
        "append" => cbor_to_struct::<CommitEvt>(row.event)
            .map(|evt| Row::Append {
                rev: evt.rev,
                since: evt.since,
            })
            .unwrap_or(Row::Other),
        _ => Row::Other,
    }
}

fn is_active_account(row: &Row) -> bool {
    matches!(row, Row::Account(None))
}

/// The creation batch the reference PDS sequences: identity, account
/// (active), a commit with no predecessor, sync.
fn is_creation_batch(rows: &[Row]) -> bool {
    matches!(
        rows,
        [Row::Identity, account, Row::Append { since: None, .. }, Row::Sync, ..]
            if is_active_account(account)
    )
}

/// The activation batch: account (active), identity, sync.
fn is_activation_batch(rows: &[Row]) -> bool {
    matches!(rows, [account, Row::Identity, Row::Sync, ..] if is_active_account(account))
}

fn first_rev_after(rows: &[Row]) -> Option<&str> {
    rows.iter().find_map(|row| match row {
        Row::Append { rev, .. } => Some(rev.as_str()),
        _ => None,
    })
}

/// Classifies the genesis and decides whether the history since it is
/// whole; `pre_deletion_max_rev` is what this server recorded before it
/// pruned the actor's earlier history, if it ever did.
fn genesis(rows: &[Row], pre_deletion_max_rev: Option<&str>) -> (GenesisKind, Result<(), String>) {
    if rows.is_empty() {
        return (
            GenesisKind::Unknown,
            Err("no publication history".to_owned()),
        );
    }
    if is_creation_batch(rows) {
        return (GenesisKind::Creation, Ok(()));
    }
    if is_activation_batch(rows) {
        return (
            GenesisKind::Activation,
            Err(
                "the account was migrated in; its earlier history was published elsewhere"
                    .to_owned(),
            ),
        );
    }
    if let [Row::Account(Some(LexiconAccountStatus::Deleted)), rest @ ..] = rows {
        if is_activation_batch(rest) {
            let verdict = match (pre_deletion_max_rev, first_rev_after(rest)) {
                (None, _) => Err("no revision maximum was recorded before the deletion".to_owned()),
                (Some(_), None) => Err("the re-created repository has published no commit".to_owned()),
                (Some(max), Some(first)) if first > max => Ok(()),
                (Some(max), Some(first)) => Err(format!(
                    "the re-created repository's first commit {first} is not above the pre-deletion maximum {max}"
                )),
            };
            return (GenesisKind::PostDeletion, verdict);
        }
    }
    (
        GenesisKind::Unknown,
        Err("no recognisable genesis".to_owned()),
    )
}

fn normalize_endpoint(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_ascii_lowercase()
}

/// Whether every hosting endpoint the DID's identity history ever named is
/// this server. A `did:web` has no auditable history and an unreachable
/// directory proves nothing.
pub async fn lifetime(plc: &plc::Client, public_url: &str, did: &str) -> Lifetime {
    if !did.starts_with("did:plc:") {
        return Lifetime::Unknown;
    }
    let Ok(log) = plc.get_audit_log(&did.to_owned()).await else {
        return Lifetime::Unknown;
    };
    let here = normalize_endpoint(public_url);
    let mut endpoints = log
        .iter()
        .filter(|entry| !entry.nullified)
        .filter_map(|entry| entry.pds_endpoint().map(normalize_endpoint))
        .peekable();
    if endpoints.peek().is_none() {
        return Lifetime::Unknown;
    }
    if endpoints.all(|endpoint| endpoint == here) {
        Lifetime::HostedHereOnly
    } else {
        Lifetime::Mixed
    }
}

pub async fn publication_frontier(
    actor_store: &ActorStore,
    sequencer: &SharedSequencer,
    lifecycle: &LifecycleStore,
    plc: &plc::Client,
    public_url: &str,
    did: &str,
) -> Result<PublicationFrontier> {
    let rows: Vec<Row> = sequencer
        .sequencer
        .read()
        .await
        .rows_for_did_after(did, 0)
        .await?
        .into_iter()
        .map(classify)
        .collect();
    let watermark = lifecycle.frontier_watermark(did).await?.unwrap_or_default();
    let publication_max_rev = rows
        .iter()
        .filter_map(|row| match row {
            Row::Append { rev, .. } => Some(rev.clone()),
            _ => None,
        })
        .chain(watermark.max_rev.clone())
        .max();

    let (current_commit_cid, signed_commit_rev, repo_root_rev) = if actor_store.exists(did).await? {
        let reader = actor_store
            .read(did.to_owned(), crate::actor_store::blobstore::unavailable())
            .await?;
        let root = reader.storage.read().await.get_root_detailed().await.ok();
        match root {
            Some(root) => {
                let signed = Repo::load(reader.storage.clone(), Some(root.cid))
                    .await
                    .map(|repo| repo.commit.rev)
                    .ok();
                (Some(root.cid.to_string()), signed, Some(root.rev))
            }
            None => (None, None, None),
        }
    } else {
        (None, None, None)
    };

    let deleting = lifecycle
        .tombstone_of(did)
        .await?
        .is_some_and(|tombstone| tombstone.logically_deleted_at.is_none())
        || lifecycle.purge_obligation_of(did).await?.is_some();
    let lifetime = lifetime(plc, public_url, did).await;
    let (genesis_kind, verdict) = genesis(&rows, watermark.pre_deletion_max_rev.as_deref());
    let reason = if deleting {
        Some("the account's deletion is in progress or its objects await purge".to_owned())
    } else if lifetime != Lifetime::HostedHereOnly {
        Some(match lifetime {
            Lifetime::Mixed => "the identity history names another host".to_owned(),
            _ => "the identity history could not be established".to_owned(),
        })
    } else {
        verdict.err()
    };
    Ok(PublicationFrontier {
        did: did.to_owned(),
        publication_max_rev,
        exposed_max_rev: watermark.exposed_max_rev,
        current_commit_cid,
        signed_commit_rev,
        repo_root_rev,
        restore_event_count: lifecycle.restore_event_count(did).await?,
        lifetime,
        genesis_kind,
        complete: reason.is_none(),
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append(rev: &str, since: Option<&str>) -> Row {
        Row::Append {
            rev: rev.to_owned(),
            since: since.map(str::to_owned),
        }
    }

    fn active() -> Row {
        Row::Account(None)
    }

    #[test]
    fn genesis_classification() {
        let creation = [
            Row::Identity,
            active(),
            append("3a", None),
            Row::Sync,
            append("3b", Some("3a")),
        ];
        assert_eq!(genesis(&creation, None), (GenesisKind::Creation, Ok(())));

        let migrated = [active(), Row::Identity, Row::Sync, append("3b", Some("3a"))];
        let (kind, verdict) = genesis(&migrated, None);
        assert_eq!(kind, GenesisKind::Activation);
        assert!(verdict.unwrap_err().contains("migrated"));

        let recreated = [
            Row::Account(Some(LexiconAccountStatus::Deleted)),
            active(),
            Row::Identity,
            Row::Sync,
            append("3c", Some("3b")),
        ];
        assert_eq!(
            genesis(&recreated, Some("3b")),
            (GenesisKind::PostDeletion, Ok(()))
        );
        let (kind, verdict) = genesis(&recreated, Some("3d"));
        assert_eq!(kind, GenesisKind::PostDeletion);
        assert!(verdict.unwrap_err().contains("not above"));
        assert!(genesis(&recreated, None)
            .1
            .unwrap_err()
            .contains("no revision maximum"));
        let no_commit = &recreated[..4];
        assert!(genesis(no_commit, Some("3b"))
            .1
            .unwrap_err()
            .contains("no commit"));

        assert_eq!(genesis(&[], None).0, GenesisKind::Unknown);
        let deleted_only = [Row::Account(Some(LexiconAccountStatus::Deleted))];
        assert_eq!(genesis(&deleted_only, Some("3a")).0, GenesisKind::Unknown);
        let odd = [Row::Other, Row::Sync];
        assert!(genesis(&odd, None).1.unwrap_err().contains("recognisable"));
        // a deactivated account event is not an activation
        let deactivated = [
            Row::Account(Some(LexiconAccountStatus::Deactivated)),
            Row::Identity,
            Row::Sync,
        ];
        assert_eq!(genesis(&deactivated, None).0, GenesisKind::Unknown);
    }

    use crate::actor_store::blobstore::MemoryBlobStore;
    use crate::background::BackgroundQueue;
    use crate::config::ActorStoreConfig;
    use crate::crawlers::Crawlers;
    use crate::sequencer::Sequencer;
    use std::io::{Read, Write};
    use std::sync::Arc;

    /// Answers every audit-log request with `body`, or 404 when empty.
    fn mock_directory(body: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let (status, payload) = if body.is_empty() {
                    ("404 Not Found", "{}")
                } else {
                    ("200 OK", body)
                };
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    )
                    .as_bytes(),
                );
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    const HERE: &str = r#"[{"did":"did:plc:x","cid":"a","nullified":false,"createdAt":"2026-01-01T00:00:00.000Z","operation":{"type":"plc_operation","services":{"atproto_pds":{"type":"AtprotoPersonalDataServer","endpoint":"https://pds.test/"}}}},
        {"did":"did:plc:x","cid":"b","nullified":true,"createdAt":"2026-01-02T00:00:00.000Z","operation":{"type":"plc_operation","services":{"atproto_pds":{"type":"AtprotoPersonalDataServer","endpoint":"https://elsewhere.test"}}}}]"#;
    const AWAY: &str = r#"[{"did":"did:plc:x","cid":"a","nullified":false,"createdAt":"2026-01-01T00:00:00.000Z","operation":{"type":"create","service":"https://pds.test"}},
        {"did":"did:plc:x","cid":"b","nullified":false,"createdAt":"2026-01-02T00:00:00.000Z","operation":{"type":"plc_operation","services":{"atproto_pds":{"type":"AtprotoPersonalDataServer","endpoint":"https://elsewhere.test"}}}}]"#;
    const NO_HOST: &str = r#"[{"did":"did:plc:x","cid":"a","nullified":false,"createdAt":"2026-01-01T00:00:00.000Z","operation":{"type":"plc_operation","services":{}}}]"#;

    #[tokio::test]
    async fn lifetime_follows_the_audit_log() {
        let here = plc::Client::new(mock_directory(HERE));
        assert_eq!(
            lifetime(&here, "https://pds.test", "did:plc:x").await,
            Lifetime::HostedHereOnly
        );
        assert_eq!(
            lifetime(&here, "https://pds.test", "did:web:pds.test").await,
            Lifetime::Unknown
        );
        let away = plc::Client::new(mock_directory(AWAY));
        assert_eq!(
            lifetime(&away, "https://pds.test", "did:plc:x").await,
            Lifetime::Mixed
        );
        let silent = plc::Client::new(mock_directory(NO_HOST));
        assert_eq!(
            lifetime(&silent, "https://pds.test", "did:plc:x").await,
            Lifetime::Unknown
        );
        let missing = plc::Client::new(mock_directory(""));
        assert_eq!(
            lifetime(&missing, "https://pds.test", "did:plc:x").await,
            Lifetime::Unknown
        );
        let down = plc::Client::new("http://127.0.0.1:1".to_owned());
        assert_eq!(
            lifetime(&down, "https://pds.test", "did:plc:x").await,
            Lifetime::Unknown
        );
    }

    #[tokio::test]
    async fn frontier_reports_the_actor_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let lifecycle = LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
        let sequencer = SharedSequencer {
            sequencer: tokio::sync::RwLock::new(Sequencer::new(
                crate::sequencer::db::get_migrated_db(dir.path().join("sequencer.sqlite"))
                    .await
                    .unwrap(),
                Crawlers::new("pds.test".to_owned(), vec![]),
                None,
            )),
        };
        let actor_store = ActorStore::new(
            &ActorStoreConfig {
                directory: dir.path().join("actors").to_str().unwrap().to_owned(),
                cache_size: 10,
            },
            BackgroundQueue::default(),
            lifecycle.clone(),
        );
        let plc = plc::Client::new(mock_directory(HERE));
        let did = "did:plc:x";

        // nothing known yet
        let empty = publication_frontier(
            &actor_store,
            &sequencer,
            &lifecycle,
            &plc,
            "https://pds.test",
            did,
        )
        .await
        .unwrap();
        assert!(!empty.complete);
        assert_eq!(empty.genesis_kind, GenesisKind::Unknown);
        assert_eq!(empty.current_commit_cid, None);
        assert_eq!(empty.reason.as_deref(), Some("no publication history"));

        // a store with no repository yet names no commit
        let keypair = secp256k1::Keypair::from_secret_key(
            &secp256k1::Secp256k1::new(),
            &secp256k1::SecretKey::from_slice(&[5u8; 32]).unwrap(),
        );
        actor_store.create(did, &keypair).await.unwrap();
        let bare = publication_frontier(
            &actor_store,
            &sequencer,
            &lifecycle,
            &plc,
            "https://pds.test",
            did,
        )
        .await
        .unwrap();
        assert_eq!(bare.current_commit_cid, None);
        assert_eq!(bare.repo_root_rev, None);

        // a repository created and published here
        let blobstore = Arc::new(MemoryBlobStore::default());
        let txn = actor_store
            .transact(did.to_owned(), blobstore.clone())
            .await
            .unwrap();
        let init = txn.create_repo(vec![], true).await.unwrap();
        drop(txn);
        {
            let mut lock = sequencer.sequencer.write().await;
            lock.sequence_identity_evt(did.to_owned(), None)
                .await
                .unwrap();
            lock.sequence_account_evt(
                did.to_owned(),
                crate::account_manager::helpers::account::AccountStatus::Active,
            )
            .await
            .unwrap();
        }
        crate::publication::publish_pending(&actor_store, &sequencer, did, None)
            .await
            .unwrap();
        lifecycle
            .record_exposure(did, &init.commit_data.rev)
            .await
            .unwrap();
        let created = publication_frontier(
            &actor_store,
            &sequencer,
            &lifecycle,
            &plc,
            "https://pds.test",
            did,
        )
        .await
        .unwrap();
        assert!(created.complete, "{created:?}");
        assert_eq!(created.genesis_kind, GenesisKind::Creation);
        assert_eq!(created.lifetime, Lifetime::HostedHereOnly);
        assert_eq!(
            created.publication_max_rev.as_deref(),
            Some(init.commit_data.rev.as_str())
        );
        assert_eq!(
            created.exposed_max_rev.as_deref(),
            Some(init.commit_data.rev.as_str())
        );
        assert_eq!(
            created.current_commit_cid.as_deref(),
            Some(init.commit_data.cid.to_string().as_str())
        );
        assert_eq!(created.signed_commit_rev, created.repo_root_rev);
        assert_eq!(created.restore_event_count, 0);
        let json = serde_json::to_value(&created).unwrap();
        assert_eq!(json["lifetime"], "hostedHereOnly");
        assert_eq!(json["genesisKind"], "creation");
        assert!(json.get("reason").is_none());

        // hosted elsewhere at some point: never complete
        let away = plc::Client::new(mock_directory(AWAY));
        let mixed = publication_frontier(
            &actor_store,
            &sequencer,
            &lifecycle,
            &away,
            "https://pds.test",
            did,
        )
        .await
        .unwrap();
        assert!(!mixed.complete);
        assert!(mixed.reason.unwrap().contains("another host"));
        let down = plc::Client::new("http://127.0.0.1:1".to_owned());
        let unknown = publication_frontier(
            &actor_store,
            &sequencer,
            &lifecycle,
            &down,
            "https://pds.test",
            did,
        )
        .await
        .unwrap();
        assert!(unknown.reason.unwrap().contains("could not be established"));

        // a deletion in progress excludes the actor
        lifecycle.tombstone(did).await.unwrap();
        let deleting = publication_frontier(
            &actor_store,
            &sequencer,
            &lifecycle,
            &plc,
            "https://pds.test",
            did,
        )
        .await
        .unwrap();
        assert!(!deleting.complete);
        assert!(deleting.reason.unwrap().contains("deletion"));
    }

    #[test]
    fn rows_classify_from_their_events() {
        use crate::models::models::RepoSeq;
        use crate::sequencer::events::{AccountEvt, SyncEvt};
        let account = RepoSeq::new(
            "did:plc:a".to_owned(),
            "account".to_owned(),
            rsky_common::struct_to_cbor(&AccountEvt {
                did: "did:plc:a".to_owned(),
                active: false,
                status: Some(LexiconAccountStatus::Deleted),
            })
            .unwrap(),
            rsky_common::now(),
        );
        assert_eq!(
            classify(account),
            Row::Account(Some(LexiconAccountStatus::Deleted))
        );
        let bad_account = RepoSeq::new(
            "did:plc:a".to_owned(),
            "account".to_owned(),
            vec![0xff],
            rsky_common::now(),
        );
        assert_eq!(classify(bad_account), Row::Other);
        let bad_append = RepoSeq::new(
            "did:plc:a".to_owned(),
            "append".to_owned(),
            vec![0xff],
            rsky_common::now(),
        );
        assert_eq!(classify(bad_append), Row::Other);
        let sync = RepoSeq::new(
            "did:plc:a".to_owned(),
            "sync".to_owned(),
            rsky_common::struct_to_cbor(&SyncEvt {
                did: "did:plc:a".to_owned(),
                blocks: vec![],
                rev: "3a".to_owned(),
            })
            .unwrap(),
            rsky_common::now(),
        );
        assert_eq!(classify(sync), Row::Sync);
        let handle = RepoSeq::new(
            "did:plc:a".to_owned(),
            "handle".to_owned(),
            vec![],
            rsky_common::now(),
        );
        assert_eq!(classify(handle), Row::Other);
        assert_eq!(
            normalize_endpoint("https://Pds.Example/"),
            "https://pds.example"
        );
    }
}
