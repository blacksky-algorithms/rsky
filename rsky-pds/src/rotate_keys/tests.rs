use super::*;
use crate::account_manager::CreateAccountOpts;
use crate::config::{ActorStoreConfig, BlobstoreConfig};
use crate::crawlers::Crawlers;
use crate::plc::test_support::MockPlc;
use aws_config::SdkConfig;
use lexicon_cid::Cid;
use std::collections::BTreeMap;
use std::str::FromStr;
use tempfile::TempDir;

const TEST_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";

fn keypair(byte: u8) -> Keypair {
    let secret = SecretKey::from_slice(&[byte; 32]).unwrap();
    Keypair::from_secret_key(&Secp256k1::new(), &secret)
}

struct Harness {
    _dir: TempDir,
    actor_store: ActorStore,
    account_manager: AccountManager,
    blobstore_factory: BlobstoreFactory,
    sequencer: RwLock<Sequencer>,
    plc: MockPlc,
    plc_client: plc::Client,
    plc_rotation_secret: SecretKey,
    shared_signing_key: Keypair,
}

impl Harness {
    fn ctx(&self) -> RotateKeysContext<'_> {
        RotateKeysContext {
            actor_store: &self.actor_store,
            account_manager: &self.account_manager,
            blobstore_factory: &self.blobstore_factory,
            sequencer: &self.sequencer,
            plc_client: &self.plc_client,
            plc_rotation_key: &self.plc_rotation_secret,
            shared_signing_key: &self.shared_signing_key,
        }
    }

    async fn sequenced_event_types(&self) -> Vec<String> {
        let sequencer = self.sequencer.read().await;
        sequencer
            .db
            .run(|conn| {
                let mut stmt = conn.prepare("SELECT \"eventType\" FROM repo_seq ORDER BY seq")?;
                let rows = stmt
                    .query_map([], |row| row.get(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
            .unwrap()
    }

    async fn key_on_disk(&self, did: &str) -> String {
        encode_did_key(&self.actor_store.keypair(did).await.unwrap().public_key())
    }

    async fn recorded_repo_root(&self, did: &str) -> Option<String> {
        let did = did.to_owned();
        self.account_manager
            .db
            .run(move |conn| {
                let mut stmt = conn.prepare("SELECT cid FROM repo_root WHERE did = ?1")?;
                let mut rows = stmt.query([did.as_str()])?;
                Ok(match rows.next()? {
                    Some(row) => Some(row.get::<_, String>(0)?),
                    None => None,
                })
            })
            .await
            .unwrap()
    }
}

/// A PDS with `dids` accounts, every one of them still holding the shared
/// signing key both on disk and in its published DID document.
async fn harness(dids: &[&str]) -> Harness {
    crate::account_manager::tests::init_env();
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| dir.path().join(name).to_string_lossy().to_string();

    let actor_store = ActorStore::new(
        &ActorStoreConfig {
            directory: path("actors"),
            cache_size: 8,
        },
        crate::background::BackgroundQueue::default(),
    );
    let account_manager = AccountManager::new(
        crate::account_manager::db::get_migrated_db(dir.path().join("account.sqlite"))
            .await
            .unwrap(),
    );
    let sequencer = RwLock::new(Sequencer::new(
        crate::sequencer::db::get_migrated_db(dir.path().join("sequencer.sqlite"))
            .await
            .unwrap(),
        Crawlers::new("pds.test".to_owned(), vec![]),
        None,
    ));
    let blobstore_factory = BlobstoreFactory::new(
        BlobstoreConfig::Disk {
            location: path("blobs"),
            tmp_location: None,
        },
        SdkConfig::builder().build(),
    );

    let plc_rotation_key = keypair(0x11);
    let shared_signing_key = keypair(0x22);
    let shared_did_key = encode_did_key(&shared_signing_key.public_key());
    let mut published = BTreeMap::new();
    for did in dids {
        published.insert((*did).to_owned(), shared_did_key.clone());
        actor_store.create(did, &shared_signing_key).await.unwrap();
        let actor_txn = actor_store
            .transact(
                (*did).to_owned(),
                blobstore_factory.blobstore((*did).to_owned()),
            )
            .await
            .unwrap();
        actor_txn.create_repo(Vec::new()).await.unwrap();
        drop(actor_txn);
        account_manager
            .create_account(CreateAccountOpts {
                did: (*did).to_owned(),
                handle: format!("{}.test", did.replace(':', "-")),
                email: Some(format!("{}@example.com", did.replace(':', "-"))),
                password: Some("password123".to_owned()),
                repo_cid: Cid::from_str(TEST_CID).unwrap(),
                repo_rev: "3jzfcijpj2z2a".to_owned(),
                invite_code: None,
                deactivated: None,
            })
            .await
            .unwrap();
    }
    let plc = MockPlc::start(&encode_did_key(&plc_rotation_key.public_key()), published);
    let plc_client = plc::Client::new(plc.url.clone());

    Harness {
        _dir: dir,
        actor_store,
        account_manager,
        blobstore_factory,
        sequencer,
        plc,
        plc_client,
        plc_rotation_secret: plc_rotation_key.secret_key(),
        shared_signing_key,
    }
}

#[tokio::test]
async fn lists_every_account_did_in_order() {
    let h = harness(&["did:plc:bob", "did:plc:alice"]).await;
    assert_eq!(
        list_account_dids(&h.account_manager).await.unwrap(),
        vec!["did:plc:alice".to_owned(), "did:plc:bob".to_owned()]
    );
}

#[tokio::test]
async fn rotates_an_account_onto_its_own_key() {
    let h = harness(&["did:plc:alice"]).await;
    let shared = encode_did_key(&h.shared_signing_key.public_key());
    assert_eq!(h.key_on_disk("did:plc:alice").await, shared);

    let report = rotate_keys(
        &h.ctx(),
        RotateKeysOpts {
            dids: None,
            dry_run: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        report,
        RotateKeysReport {
            scanned: 1,
            rotated: 1,
            skipped: 0,
            failed: 0,
        }
    );

    // the key file, the published document and the PLC operation all agree,
    // and none of them is the shared key any more
    let rotated = h.key_on_disk("did:plc:alice").await;
    assert_ne!(rotated, shared);
    assert_eq!(h.plc.published_key("did:plc:alice"), Some(rotated.clone()));
    let posted = h.plc.posted();
    assert_eq!(posted.len(), 1);
    assert_eq!(
        posted[0]["verificationMethods"]["atproto"].as_str(),
        Some(rotated.as_str())
    );

    // the empty commit was sequenced as identity + sync, in that order
    assert_eq!(
        h.sequenced_event_types().await,
        vec!["identity".to_owned(), "sync".to_owned()]
    );

    // the account db's repo root moved off the placeholder written at signup
    let root = h.recorded_repo_root("did:plc:alice").await;
    assert!(root.is_some());
    assert_ne!(root, Some(TEST_CID.to_owned()));
}

#[tokio::test]
async fn a_second_run_is_a_no_op() {
    let h = harness(&["did:plc:alice"]).await;
    rotate_keys(
        &h.ctx(),
        RotateKeysOpts {
            dids: None,
            dry_run: false,
        },
    )
    .await
    .unwrap();
    let rotated = h.key_on_disk("did:plc:alice").await;

    let report = rotate_keys(
        &h.ctx(),
        RotateKeysOpts {
            dids: None,
            dry_run: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        report,
        RotateKeysReport {
            scanned: 1,
            rotated: 0,
            skipped: 1,
            failed: 0,
        }
    );
    assert_eq!(h.key_on_disk("did:plc:alice").await, rotated);
    assert_eq!(h.plc.posted().len(), 1);
}

#[tokio::test]
async fn a_run_interrupted_after_the_key_write_republishes_that_key() {
    let h = harness(&["did:plc:alice"]).await;
    // stand in for a crash between the key write and the PLC operation
    let written = keypair(0x44);
    h.actor_store
        .set_keypair("did:plc:alice", &written)
        .await
        .unwrap();

    let outcome = rotate_one(&h.ctx(), "did:plc:alice", false).await.unwrap();
    assert_eq!(outcome, RotationOutcome::Rotated);
    // the interrupted run's key is published rather than a third key
    let expected = encode_did_key(&written.public_key());
    assert_eq!(h.key_on_disk("did:plc:alice").await, expected);
    assert_eq!(h.plc.published_key("did:plc:alice"), Some(expected));
}

#[tokio::test]
async fn a_dry_run_touches_nothing() {
    let h = harness(&["did:plc:alice"]).await;
    let shared = encode_did_key(&h.shared_signing_key.public_key());
    let report = rotate_keys(
        &h.ctx(),
        RotateKeysOpts {
            dids: None,
            dry_run: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        report,
        RotateKeysReport {
            scanned: 1,
            rotated: 1,
            skipped: 0,
            failed: 0,
        }
    );
    assert_eq!(h.key_on_disk("did:plc:alice").await, shared);
    assert_eq!(h.plc.published_key("did:plc:alice"), Some(shared));
    assert!(h.plc.posted().is_empty());
    assert!(h.sequenced_event_types().await.is_empty());
}

#[tokio::test]
async fn non_plc_dids_are_skipped() {
    let h = harness(&["did:web:alice.test"]).await;
    let report = rotate_keys(
        &h.ctx(),
        RotateKeysOpts {
            dids: None,
            dry_run: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        report,
        RotateKeysReport {
            scanned: 1,
            rotated: 0,
            skipped: 1,
            failed: 0,
        }
    );
    assert_eq!(
        rotate_one(&h.ctx(), "did:web:alice.test", false)
            .await
            .unwrap(),
        RotationOutcome::Skipped(SkipReason::NotPlcDid)
    );
}

#[tokio::test]
async fn an_explicit_did_list_bounds_the_run() {
    let h = harness(&["did:plc:alice", "did:plc:bob"]).await;
    let report = rotate_keys(
        &h.ctx(),
        RotateKeysOpts {
            dids: Some(vec!["did:plc:alice".to_owned()]),
            dry_run: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        report,
        RotateKeysReport {
            scanned: 1,
            rotated: 1,
            skipped: 0,
            failed: 0,
        }
    );
    let shared = encode_did_key(&h.shared_signing_key.public_key());
    assert_eq!(h.key_on_disk("did:plc:bob").await, shared);
}

#[tokio::test]
async fn a_failing_did_is_counted_and_the_run_continues() {
    let h = harness(&["did:plc:alice"]).await;
    let report = rotate_keys(
        &h.ctx(),
        RotateKeysOpts {
            // no key file and no account for this one
            dids: Some(vec!["did:plc:ghost".to_owned(), "did:plc:alice".to_owned()]),
            dry_run: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        report,
        RotateKeysReport {
            scanned: 2,
            rotated: 1,
            skipped: 0,
            failed: 1,
        }
    );
    assert_ne!(
        h.key_on_disk("did:plc:alice").await,
        encode_did_key(&h.shared_signing_key.public_key())
    );
}
