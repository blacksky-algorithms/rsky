use super::*;
use crate::actor_store::blobstore::MemoryBlobStore;
use rsky_repo::types::PreparedDelete;

const TEST_DID: &str = "did:example:alice";
const TEST_SECRET_HEX: &str = "1d2f8064213bd212453fa93943c084dbbf42104d02f1f02b23a638f9a48f925a";
const REPLACEMENT_SECRET_HEX: &str =
    "5d2f8064213bd212453fa93943c084dbbf42104d02f1f02b23a638f9a48f925a";

fn cid(value: &str) -> Cid {
    Cid::from_str(value).unwrap()
}

fn current() -> Cid {
    cid("bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku")
}

fn other() -> Cid {
    cid("bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4")
}

fn test_keypair() -> Keypair {
    import_keypair(&hex::decode(TEST_SECRET_HEX).unwrap()).unwrap()
}

async fn test_store(cache_size: usize) -> (tempfile::TempDir, ActorStore) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = ActorStoreConfig {
        directory: dir.path().join("actors").to_string_lossy().to_string(),
        cache_size,
    };
    let lifecycle = LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
        .await
        .unwrap();
    let store = ActorStore::new(&cfg, BackgroundQueue::default(), lifecycle);
    (dir, store)
}

fn blobstore() -> Arc<MemoryBlobStore> {
    Arc::new(MemoryBlobStore::default())
}

fn post_write(rkey: &str, text: &str) -> PreparedCreateOrUpdate {
    let record: rsky_repo::types::RepoRecord = serde_json::from_value(serde_json::json!({
        "$type": "app.bsky.feed.post",
        "text": text,
        "createdAt": "2023-01-01T00:00:00.000Z",
    }))
    .unwrap();
    let cid = rsky_common::ipld::cid_for_cbor(&record).unwrap();
    PreparedCreateOrUpdate {
        action: WriteOpAction::Create,
        uri: format!("at://{TEST_DID}/app.bsky.feed.post/{rkey}"),
        cid,
        swap_cid: None,
        record,
        blobs: vec![],
    }
}

#[test]
fn no_swap_cid_skips_checks() {
    assert!(check_record_swap(&WriteOpAction::Update, &Some(current()), &None).is_ok());
    assert!(check_record_swap(&WriteOpAction::Delete, &None, &None).is_ok());
    assert!(check_record_swap(&WriteOpAction::Create, &None, &None).is_ok());
}

#[test]
fn create_with_swap_cid_is_rejected() {
    assert!(matches!(
        check_record_swap(&WriteOpAction::Create, &None, &Some(current())),
        Err(FormatCommitError::BadRecordSwap(_))
    ));
}

#[test]
fn matching_swap_cid_is_accepted() {
    assert!(check_record_swap(&WriteOpAction::Update, &Some(current()), &Some(current())).is_ok());
    assert!(check_record_swap(&WriteOpAction::Delete, &Some(current()), &Some(current())).is_ok());
}

#[test]
fn mismatched_or_missing_current_record_is_rejected() {
    assert!(matches!(
        check_record_swap(&WriteOpAction::Update, &Some(current()), &Some(other())),
        Err(FormatCommitError::RecordSwapMismatch(_))
    ));
    assert!(matches!(
        check_record_swap(&WriteOpAction::Delete, &None, &Some(current())),
        Err(FormatCommitError::RecordSwapMismatch(_))
    ));
}

#[test]
fn format_commit_errors_display() {
    assert!(FormatCommitError::BadRecordSwap("x".to_owned())
        .to_string()
        .contains("BadRecordSwapError"));
    assert!(FormatCommitError::RecordSwapMismatch("x".to_owned())
        .to_string()
        .contains("current record"));
    assert!(FormatCommitError::BadCommitSwap("cid".to_owned())
        .to_string()
        .contains("BadCommitSwapError"));
    assert!(FormatCommitError::MissingRepoRoot("did".to_owned())
        .to_string()
        .contains("No repo root"));
}

#[test]
fn rejects_unsafe_path_parts() {
    assert!(assert_safe_path_part("did:example:alice").is_ok());
    assert!(assert_safe_path_part("").is_err());
    assert!(assert_safe_path_part(".hidden").is_err());
    assert!(assert_safe_path_part("a/b").is_err());
    assert!(assert_safe_path_part("a\\b").is_err());
    assert!(assert_safe_path_part("a..b").is_err());
}

#[tokio::test]
async fn create_open_keypair_destroy_roundtrip() {
    let (_dir, store) = test_store(10).await;
    let keypair = test_keypair();

    assert!(!store.exists(TEST_DID).await.unwrap());
    assert!(store.keypair(TEST_DID).await.is_err());
    assert!(store.read(TEST_DID.to_owned(), blobstore()).await.is_err());
    assert!(store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .is_err());

    store.create(TEST_DID, &keypair).await.unwrap();
    assert!(store.exists(TEST_DID).await.unwrap());
    // creating again fails
    assert!(store.create(TEST_DID, &keypair).await.is_err());

    let loaded = store.keypair(TEST_DID).await.unwrap();
    assert_eq!(loaded.secret_bytes(), keypair.secret_bytes());

    let reader = store.read(TEST_DID.to_owned(), blobstore()).await.unwrap();
    assert_eq!(reader.did, TEST_DID);
    assert_eq!(
        reader.keypair().await.unwrap().secret_bytes(),
        keypair.secret_bytes()
    );
    assert!(reader.get_repo_root().await.is_none());

    store.destroy(TEST_DID, blobstore()).await.unwrap();
    assert!(!store.exists(TEST_DID).await.unwrap());
    // destroying a missing actor is a no-op
    store.destroy(TEST_DID, blobstore()).await.unwrap();
}

#[tokio::test]
async fn location_shards_by_did_hash() {
    let (_dir, store) = test_store(10).await;
    let location = store.get_location(TEST_DID).unwrap();
    let did_hash = hex::encode(Sha256::digest(TEST_DID.as_bytes()));
    assert!(location.directory.to_string_lossy().contains(&format!(
        "{}/{}",
        &did_hash[0..2],
        TEST_DID
    )));
    assert_eq!(
        location.db_location.file_name().unwrap().to_str().unwrap(),
        "store.sqlite"
    );
    assert_eq!(
        location.key_location.file_name().unwrap().to_str().unwrap(),
        "key"
    );
    assert!(store.get_location("bad/did").is_err());
}

#[tokio::test]
async fn concurrent_transactions_serialize_per_did() {
    let (_dir, store) = test_store(10).await;
    let store = Arc::new(store);
    store.create(TEST_DID, &test_keypair()).await.unwrap();

    let running = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let max_running = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let store = store.clone();
        let running = running.clone();
        let max_running = max_running.clone();
        handles.push(tokio::spawn(async move {
            let txn = store
                .transact(TEST_DID.to_owned(), blobstore())
                .await
                .unwrap();
            let now = running.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            max_running.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            running.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            drop(txn);
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    assert_eq!(max_running.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn lru_evicts_least_recently_used_db() {
    let (_dir, store) = test_store(1).await;
    let keypair = test_keypair();
    let did_bob = "did:example:bob";
    store.create(TEST_DID, &keypair).await.unwrap();
    store.create(did_bob, &keypair).await.unwrap();

    // bob was cached last (cache size 1), so alice must be re-opened from disk
    let alice_location = store.get_location(TEST_DID).unwrap();
    tokio::fs::remove_file(&alice_location.db_location)
        .await
        .unwrap();
    assert!(store.read(TEST_DID.to_owned(), blobstore()).await.is_err());
    // bob is still served from the cache even with the file gone
    let bob_location = store.get_location(did_bob).unwrap();
    tokio::fs::remove_file(&bob_location.db_location)
        .await
        .unwrap();
    assert!(store.read(did_bob.to_owned(), blobstore()).await.is_ok());
}

#[tokio::test]
async fn reserved_keypair_lifecycle() {
    let (_dir, store) = test_store(10).await;
    // reserving without a did keys the file by the key's own did
    let key_did = store.reserve_keypair(None).await.unwrap();
    assert!(key_did.starts_with("did:key:"));
    let loaded = store.get_reserved_keypair(&key_did).await.unwrap().unwrap();
    assert_eq!(encode_did_key(&loaded.public_key()), key_did);

    // reserving for a did is idempotent
    let for_did = store.reserve_keypair(Some(TEST_DID)).await.unwrap();
    let again = store.reserve_keypair(Some(TEST_DID)).await.unwrap();
    assert_eq!(for_did, again);
    assert!(store
        .get_reserved_keypair(TEST_DID)
        .await
        .unwrap()
        .is_some());

    // clearing with a did that has no reserved file is a no-op for that file
    store
        .clear_reserved_keypair(&key_did, Some("did:example:nobody"))
        .await
        .unwrap();
    store
        .clear_reserved_keypair(&for_did, Some(TEST_DID))
        .await
        .unwrap();
    assert!(store
        .get_reserved_keypair(&key_did)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .get_reserved_keypair(TEST_DID)
        .await
        .unwrap()
        .is_none());
    // clearing again is a no-op
    store.clear_reserved_keypair(&key_did, None).await.unwrap();
    assert!(store.reserve_keypair(Some("bad/did")).await.is_err());
}

#[tokio::test]
async fn create_account_write_and_read_back_records() {
    let (_dir, store) = test_store(10).await;
    let keypair = test_keypair();
    let blobs = blobstore();
    store.create(TEST_DID, &keypair).await.unwrap();

    // initialize the repo
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobs.clone())
        .await
        .unwrap();
    let init_commit = txn.create_repo(vec![], true).await.unwrap();
    assert!(init_commit.ops.is_empty());
    assert!(init_commit.prev_data.is_none());
    let root = txn.get_repo_root().await.unwrap();
    assert_eq!(root, init_commit.commit_data.cid);

    // create a record
    let create = post_write("3jt5vlkoraa2a", "hello world");
    let create_uri: AtUri = create.uri.clone().try_into().unwrap();
    let commit = txn
        .process_writes(vec![PreparedWrite::Create(create.clone())], None)
        .await
        .unwrap();
    assert_eq!(commit.ops.len(), 1);
    assert!(matches!(commit.ops[0].action, CommitAction::Create));
    assert_eq!(commit.ops[0].cid, Some(create.cid));
    assert!(commit.ops[0].prev.is_none());
    assert!(commit.prev_data.is_some());

    let got = txn
        .record
        .get_record(&create_uri, None, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.cid, create.cid.to_string());
    assert_eq!(got.value, create.record);

    // swap-commit mismatch is rejected
    let unrelated = post_write("3jt5vlkorbb2b", "swap target");
    let bad_swap = txn
        .process_writes(
            vec![PreparedWrite::Create(unrelated.clone())],
            Some(other()),
        )
        .await;
    assert!(bad_swap.is_err());

    // update the record
    let mut update = post_write("3jt5vlkoraa2a", "hello again");
    update.action = WriteOpAction::Update;
    let update_commit = txn
        .process_writes(vec![PreparedWrite::Update(update.clone())], None)
        .await
        .unwrap();
    assert_eq!(update_commit.ops[0].prev, Some(create.cid));
    let got = txn
        .record
        .get_record(&create_uri, None, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.cid, update.cid.to_string());

    // sync event data and car stream are readable
    let sync_data = txn.get_sync_event_data().await.unwrap();
    assert_eq!(sync_data.cid, update_commit.commit_data.cid);
    drop(txn);

    let reader = store
        .read(TEST_DID.to_owned(), blobs.clone())
        .await
        .unwrap();
    {
        let storage_guard = reader.storage.read().await;
        let car = storage_guard.get_car_stream(None).await.unwrap();
        assert!(!car.is_empty());
    }

    // delete the record
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobs.clone())
        .await
        .unwrap();
    let delete = PreparedWrite::Delete(PreparedDelete {
        action: WriteOpAction::Delete,
        uri: create.uri.clone(),
        swap_cid: None,
    });
    let delete_commit = txn.process_writes(vec![delete], None).await.unwrap();
    assert!(matches!(delete_commit.ops[0].action, CommitAction::Delete));
    assert_eq!(delete_commit.ops[0].prev, Some(update.cid));
    assert!(txn
        .record
        .get_record(&create_uri, None, None)
        .await
        .unwrap()
        .is_none());

    // no duplicate cids for empty inputs
    assert!(txn
        .get_duplicate_record_cids(vec![], vec![])
        .await
        .unwrap()
        .is_empty());
    drop(txn);

    store.destroy(TEST_DID, blobs).await.unwrap();
    assert!(!store.exists(TEST_DID).await.unwrap());
}

#[tokio::test]
async fn process_writes_requires_repo_root() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    let res = txn
        .process_writes(
            vec![PreparedWrite::Create(post_write(
                "3jt5vlkoraa2a",
                "no root",
            ))],
            None,
        )
        .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn duplicate_record_cids_are_detected() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    txn.create_repo(vec![], true).await.unwrap();

    // two records with identical content share a cid
    let write_one = post_write("3jt5vlkoraa2a", "same content");
    let write_two = PreparedCreateOrUpdate {
        uri: format!("at://{TEST_DID}/app.bsky.feed.post/3jt5vlkorbb2b"),
        ..write_one.clone()
    };
    txn.process_writes(
        vec![
            PreparedWrite::Create(write_one.clone()),
            PreparedWrite::Create(write_two.clone()),
        ],
        None,
    )
    .await
    .unwrap();

    let write_one_uri: AtUri = write_one.uri.clone().try_into().unwrap();
    let dupes = txn
        .get_duplicate_record_cids(vec![write_one.cid], vec![write_one_uri])
        .await
        .unwrap();
    assert_eq!(dupes, vec![write_two.cid]);

    // deleting only one of the two keeps the shared block for the other
    let delete = PreparedWrite::Delete(PreparedDelete {
        action: WriteOpAction::Delete,
        uri: write_one.uri.clone(),
        swap_cid: None,
    });
    txn.process_writes(vec![delete], None).await.unwrap();
    let write_two_uri: AtUri = write_two.uri.clone().try_into().unwrap();
    let got = txn
        .record
        .get_record(&write_two_uri, None, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.value, write_two.record);
}

#[tokio::test]
async fn failing_blobstore_fails_every_operation() {
    use crate::actor_store::blobstore::BlobStore;
    let store = FailingBlobStore::default();
    let cid = current();
    assert!(store.put_temp(vec![]).await.is_err());
    assert!(store.make_permanent("key".to_owned(), cid).await.is_err());
    assert!(store.put_permanent(cid, vec![]).await.is_err());
    assert!(store.quarantine(cid).await.is_err());
    assert!(store.unquarantine(cid).await.is_err());
    assert!(store.get_bytes(cid).await.is_err());
    assert!(store.get_stream(cid).await.is_err());
    assert!(store.has_temp("key".to_owned()).await.is_err());
    assert!(store.has_stored(cid).await.is_err());
    assert!(store.delete(cid).await.is_err());
    assert!(store.delete_many(vec![cid]).await.is_err());
    assert!(store.delete_all().is_none());
    let failing_delete_all = FailingBlobStore {
        fail_delete_all: true,
    };
    assert!(failing_delete_all.delete_all().unwrap().await.is_err());
}

#[tokio::test]
async fn destroy_deletes_blobs_from_blobstore() {
    let (_dir, store) = test_store(10).await;
    let blobs = blobstore();
    store.create(TEST_DID, &test_keypair()).await.unwrap();

    let txn = store
        .transact(TEST_DID.to_owned(), blobs.clone())
        .await
        .unwrap();
    let metadata = txn
        .blob
        .upload_blob_and_get_metadata("text/plain".to_owned(), b"destroy me".to_vec())
        .await
        .unwrap();
    let blob_ref = txn.blob.track_untethered_blob(metadata).await.unwrap();
    let blob_cid = blob_ref.get_cid().unwrap();
    blobs
        .put_permanent(blob_cid, b"destroy me".to_vec())
        .await
        .unwrap();
    drop(txn);

    assert!(!blobs.stored_cids().is_empty());
    store.destroy(TEST_DID, blobs.clone()).await.unwrap();
    assert!(blobs.stored_cids().is_empty());
}

#[derive(Default)]
struct FailingBlobStore {
    fail_delete_all: bool,
}

impl crate::actor_store::blobstore::BlobStore for FailingBlobStore {
    fn put_temp(&self, _bytes: Vec<u8>) -> futures::future::BoxFuture<'_, Result<String>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn make_permanent(
        &self,
        _key: String,
        _cid: Cid,
    ) -> futures::future::BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn put_permanent(
        &self,
        _cid: Cid,
        _bytes: Vec<u8>,
    ) -> futures::future::BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn quarantine(&self, _cid: Cid) -> futures::future::BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn unquarantine(&self, _cid: Cid) -> futures::future::BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn get_bytes(&self, _cid: Cid) -> futures::future::BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn get_stream(
        &self,
        _cid: Cid,
    ) -> futures::future::BoxFuture<'_, Result<aws_sdk_s3::primitives::ByteStream>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn has_temp(&self, _key: String) -> futures::future::BoxFuture<'_, Result<bool>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn has_stored(&self, _cid: Cid) -> futures::future::BoxFuture<'_, Result<bool>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn delete(&self, _cid: Cid) -> futures::future::BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn delete_many(&self, _cids: Vec<Cid>) -> futures::future::BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn delete_all(&self) -> Option<futures::future::BoxFuture<'_, Result<()>>> {
        if self.fail_delete_all {
            Some(Box::pin(async { bail!("blobstore unavailable") }))
        } else {
            None
        }
    }
    fn make_permanent_copy_only(
        &self,
        _key: String,
        _cid: Cid,
    ) -> futures::future::BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn has_quarantined(&self, _cid: Cid) -> futures::future::BoxFuture<'_, Result<bool>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
    fn restore_copy_only(&self, _cid: Cid) -> futures::future::BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blobstore unavailable") })
    }
}

#[tokio::test]
async fn reopens_evicted_db_from_disk() {
    let (_dir, store) = test_store(1).await;
    let keypair = test_keypair();
    store.create(TEST_DID, &keypair).await.unwrap();
    // bob evicts alice from the single-entry cache
    store.create("did:example:bob", &keypair).await.unwrap();
    // alice re-opens from disk
    let reader = store.read(TEST_DID.to_owned(), blobstore()).await.unwrap();
    assert!(reader.get_repo_root().await.is_none());
}

#[tokio::test]
async fn destroy_logs_blobstore_failures_and_still_removes_dir() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    let metadata = txn
        .blob
        .upload_blob_and_get_metadata("text/plain".to_owned(), b"orphan".to_vec())
        .await
        .unwrap();
    txn.blob.track_untethered_blob(metadata).await.unwrap();
    drop(txn);

    store
        .destroy(TEST_DID, Arc::new(FailingBlobStore::default()))
        .await
        .unwrap();
    assert!(!store.exists(TEST_DID).await.unwrap());
}

#[tokio::test]
async fn destroy_uses_disk_delete_all_when_available() {
    use crate::actor_store::disk_blobstore::DiskBlobStore;
    let (dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let disk_store = Arc::new(DiskBlobStore::new(
        TEST_DID.to_owned(),
        &dir.path().join("blobs"),
        None,
        None,
    ));
    let bytes = b"disk destroy".to_vec();
    let cid = rsky_common::ipld::sha256_to_cid(Sha256::digest(&bytes).to_vec());
    disk_store.put_permanent(cid, bytes).await.unwrap();
    assert!(disk_store.has_stored(cid).await.unwrap());

    store.destroy(TEST_DID, disk_store.clone()).await.unwrap();
    assert!(!store.exists(TEST_DID).await.unwrap());
    assert!(!disk_store.has_stored(cid).await.unwrap());
}

#[tokio::test]
async fn destroy_logs_delete_all_failures_and_still_removes_dir() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    store
        .destroy(
            TEST_DID,
            Arc::new(FailingBlobStore {
                fail_delete_all: true,
            }),
        )
        .await
        .unwrap();
    assert!(!store.exists(TEST_DID).await.unwrap());
}

#[tokio::test]
async fn reader_keypair_errors_when_key_missing() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let location = store.get_location(TEST_DID).unwrap();
    tokio::fs::remove_file(&location.key_location)
        .await
        .unwrap();
    let reader = store.read(TEST_DID.to_owned(), blobstore()).await.unwrap();
    assert!(reader.keypair().await.is_err());
    assert!(store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .is_err());
}

#[tokio::test]
async fn reserved_key_load_errors_on_unreadable_file() {
    let (_dir, store) = test_store(10).await;
    tokio::fs::create_dir_all(store.reserved_key_dir.join("not-a-file"))
        .await
        .unwrap();
    assert!(store.get_reserved_keypair("not-a-file").await.is_err());
    assert!(store.get_reserved_keypair("bad/path").await.is_err());
}

#[tokio::test]
async fn create_repo_with_initial_writes() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    let write = post_write("3jt5vlkoraa2a", "first post");
    let commit = txn.create_repo(vec![write.clone()], true).await.unwrap();
    assert_eq!(commit.ops.len(), 1);
    assert_eq!(commit.ops[0].cid, Some(write.cid));
    let write_uri: AtUri = write.uri.clone().try_into().unwrap();
    let got = txn
        .record
        .get_record(&write_uri, None, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.cid, write.cid.to_string());

    // matching swap-commit is accepted
    let root = txn.get_repo_root().await.unwrap();
    let next = post_write("3jt5vlkorbb2b", "second post");
    txn.process_writes(vec![PreparedWrite::Create(next)], Some(root))
        .await
        .unwrap();
}

#[tokio::test]
async fn process_import_repo_applies_commit_and_writes() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    let init = txn.create_repo(vec![], true).await.unwrap();

    let write = post_write("3jt5vlkoraa2a", "imported");
    let commit = CommitData {
        cid: init.commit_data.cid,
        rev: "3jt5vlkorxx2x".to_owned(),
        since: None,
        prev: None,
        new_blocks: rsky_repo::block_map::BlockMap::new(),
        relevant_blocks: rsky_repo::block_map::BlockMap::new(),
        removed_cids: rsky_repo::cid_set::CidSet::new(None),
    };
    txn.process_import_repo(commit, vec![PreparedWrite::Create(write.clone())])
        .await
        .unwrap();
    let write_uri: AtUri = write.uri.try_into().unwrap();
    assert!(txn
        .record
        .get_record(&write_uri, None, None)
        .await
        .unwrap()
        .is_none());
    // record row indexed but block content lives outside repo_block in this synthetic import
    assert!(txn
        .record
        .has_record(write_uri.to_string(), None, None)
        .await
        .unwrap());
}

#[tokio::test]
async fn moved_record_keeps_shared_blocks() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    txn.create_repo(vec![], true).await.unwrap();

    let original = post_write("3jt5vlkoraa2a", "same content");
    txn.process_writes(vec![PreparedWrite::Create(original.clone())], None)
        .await
        .unwrap();

    // move the record: delete the old rkey and re-create the same content
    // under a new rkey in a single commit (cid stays the same)
    let moved = PreparedCreateOrUpdate {
        uri: format!("at://{TEST_DID}/app.bsky.feed.post/3jt5vlkorbb2b"),
        ..original.clone()
    };
    let delete = PreparedWrite::Delete(PreparedDelete {
        action: WriteOpAction::Delete,
        uri: original.uri.clone(),
        swap_cid: None,
    });
    let commit = txn
        .process_writes(vec![delete, PreparedWrite::Create(moved.clone())], None)
        .await
        .unwrap();
    assert_eq!(commit.ops.len(), 2);

    let moved_uri: AtUri = moved.uri.clone().try_into().unwrap();
    let got = txn
        .record
        .get_record(&moved_uri, None, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.cid, original.cid.to_string());
}

#[tokio::test]
async fn set_keypair_replaces_the_key_and_the_next_commit_uses_it() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    {
        let actor_txn = store
            .transact(TEST_DID.to_owned(), blobstore())
            .await
            .unwrap();
        actor_txn.create_repo(Vec::new(), true).await.unwrap();
    }

    let replacement = import_keypair(&hex::decode(REPLACEMENT_SECRET_HEX).unwrap()).unwrap();
    store.set_keypair(TEST_DID, &replacement).await.unwrap();

    assert_eq!(
        store.keypair(TEST_DID).await.unwrap().secret_bytes(),
        replacement.secret_bytes()
    );
    // the transactor snapshots the key at open time, so it picks up the swap
    let actor_txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    assert_eq!(actor_txn.keypair.secret_bytes(), replacement.secret_bytes());
}

#[tokio::test]
async fn set_keypair_leaves_no_temp_file_behind() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let location = store.get_location(TEST_DID).unwrap();
    store
        .set_keypair(
            TEST_DID,
            &import_keypair(&hex::decode(REPLACEMENT_SECRET_HEX).unwrap()).unwrap(),
        )
        .await
        .unwrap();

    let mut entries = tokio::fs::read_dir(&location.directory).await.unwrap();
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        names.push(entry.file_name().to_string_lossy().to_string());
    }
    assert!(names.contains(&"key".to_owned()), "{names:?}");
    assert!(
        !names.iter().any(|name| name.ends_with(".tmp")),
        "{names:?}"
    );
}

#[tokio::test]
async fn set_keypair_rejects_an_unsafe_did() {
    let (_dir, store) = test_store(10).await;
    assert!(store.set_keypair("bad/did", &test_keypair()).await.is_err());
}

#[tokio::test]
async fn atomic_write_key_rejects_a_pathless_location() {
    assert!(atomic_write_key(Path::new("/"), b"bytes").await.is_err());
}

async fn store_tables(store: &ActorStore, did: &str) -> Vec<String> {
    let db = store.open_db(did, OpenMode::Read).await.unwrap();
    db.run(|conn| {
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<String>, rusqlite::Error>>()?;
        Ok(names)
    })
    .await
    .unwrap()
}

/// Lays down a store the way the reference PDS creates one: only the shared
/// schema, tracked in Kysely's ledger, no rsky-local tables.
async fn reference_store(store: &ActorStore, did: &str) {
    let location = store.get_location(did).unwrap();
    tokio::fs::create_dir_all(&location.directory)
        .await
        .unwrap();
    tokio::fs::write(&location.key_location, test_keypair().secret_bytes())
        .await
        .unwrap();
    let db = crate::actor_store::db::get_db(&location.db_location).unwrap();
    db.run(|conn| {
        conn.execute_batch(crate::actor_store::db::ACTOR_DB_MIGRATIONS[0].sql)?;
        conn.execute_batch(
            "CREATE TABLE kysely_migration (name varchar(255) NOT NULL PRIMARY KEY, \
                timestamp varchar(255) NOT NULL);\
             CREATE TABLE kysely_migration_lock (id varchar(255) NOT NULL PRIMARY KEY, \
                is_locked integer NOT NULL DEFAULT 0);\
             INSERT INTO kysely_migration_lock VALUES ('migration_lock', 0);\
             INSERT INTO kysely_migration VALUES ('001', '2026-01-01T00:00:00.000Z');",
        )?;
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn reading_a_reference_store_never_changes_its_schema() {
    let (_dir, store) = test_store(4).await;
    reference_store(&store, TEST_DID).await;
    let before = store_tables(&store, TEST_DID).await;
    assert!(!before.contains(&"migrations".to_string()));
    assert!(!before.contains(&"space_repo".to_string()));
    let reader = store.read(TEST_DID.to_string(), blobstore()).await.unwrap();
    assert!(reader.get_repo_root().await.is_none());
    assert_eq!(store_tables(&store, TEST_DID).await, before);
}

#[tokio::test]
async fn writing_a_reference_store_applies_only_local_migrations() {
    let (_dir, store) = test_store(4).await;
    reference_store(&store, TEST_DID).await;
    let _tx = store
        .transact(TEST_DID.to_string(), blobstore())
        .await
        .unwrap();
    let tables = store_tables(&store, TEST_DID).await;
    assert!(tables.contains(&"migrations".to_string()));
    assert!(tables.contains(&"space_repo".to_string()));
    let (shared, local): (Vec<String>, Vec<String>) = store
        .open_db(TEST_DID, OpenMode::Read)
        .await
        .unwrap()
        .run(|conn| {
            fn names(
                conn: &rusqlite::Connection,
                sql: &str,
            ) -> Result<Vec<String>, rusqlite::Error> {
                let mut stmt = conn.prepare(sql)?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect()
            }
            Ok((
                names(conn, "SELECT name FROM kysely_migration ORDER BY name")?,
                names(conn, "SELECT name FROM migrations ORDER BY name")?,
            ))
        })
        .await
        .unwrap();
    assert_eq!(shared, ["001"]);
    assert_eq!(local, ["002", "003", "004", "005"]);
}

#[tokio::test]
async fn a_cached_read_handle_is_migrated_before_the_first_write() {
    let (_dir, store) = test_store(4).await;
    reference_store(&store, TEST_DID).await;
    store.read(TEST_DID.to_string(), blobstore()).await.unwrap();
    assert!(!store_tables(&store, TEST_DID)
        .await
        .contains(&"space_repo".to_string()));
    store
        .transact(TEST_DID.to_string(), blobstore())
        .await
        .unwrap();
    assert!(store_tables(&store, TEST_DID)
        .await
        .contains(&"space_repo".to_string()));
}

#[tokio::test]
async fn write_limits_match_the_reference() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    txn.create_repo(vec![], true).await.unwrap();

    let too_many: Vec<PreparedWrite> = (0..201)
        .map(|i| PreparedWrite::Create(post_write(&format!("3jt5vlkor{i:04}"), "x")))
        .collect();
    let err = txn.process_writes(too_many, None).await.unwrap_err();
    assert_eq!(
        err.downcast_ref::<WriteLimitError>(),
        Some(&WriteLimitError::TooManyWrites)
    );

    let huge = post_write("3jt5vlkorhuge", &"x".repeat(2_100_000));
    let err = txn
        .process_writes(vec![PreparedWrite::Create(huge)], None)
        .await
        .unwrap_err();
    assert_eq!(
        err.downcast_ref::<WriteLimitError>(),
        Some(&WriteLimitError::EventTooLarge)
    );
    assert_eq!(
        WriteLimitError::TooManyWrites.to_string(),
        "Too many writes. Max: 200"
    );
    // a rejected write leaves no intent behind
    assert_eq!(txn.pending_intents().await.unwrap().len(), 2);
}

#[tokio::test]
async fn an_import_replaces_the_repository_without_an_intent() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    let init = txn.create_repo(vec![], true).await.unwrap();
    let write = post_write("3jt5vlkorimp1", "imported");
    let (commit, _) = txn
        .format_commit(vec![PreparedWrite::Create(write.clone())], None)
        .await
        .unwrap();
    txn.process_import_repo(
        commit.commit_data.clone(),
        vec![PreparedWrite::Create(write.clone())],
    )
    .await
    .unwrap();
    let root = txn.get_repo_root().await.unwrap();
    assert_eq!(root, commit.commit_data.cid);
    assert_ne!(root, init.commit_data.cid);
    let uri: AtUri = write.uri.try_into().unwrap();
    assert!(txn
        .record
        .get_record(&uri, None, None)
        .await
        .unwrap()
        .is_some());
    // the import published nothing, and the creation batch it replaced
    // must never be published behind the imported head
    let intents = txn.all_intents().await.unwrap();
    assert_eq!(intents.len(), 2);
    assert!(intents
        .iter()
        .all(|intent| intent.cid == init.commit_data.cid.to_string()));
    assert!(intents.iter().all(|intent| intent.state == "superseded"));
    assert!(txn.pending_intents().await.unwrap().is_empty());

    // an import into an empty store creates the root
    let empty_did = "did:example:empty";
    store.create(empty_did, &test_keypair()).await.unwrap();
    let mut empty = store
        .transact(empty_did.to_owned(), blobstore())
        .await
        .unwrap();
    let mut fresh = Repo::format_init_commit(
        empty.storage.clone(),
        empty_did.to_owned(),
        &empty.keypair,
        None,
    )
    .await
    .unwrap();
    fresh.since = None;
    empty
        .process_import_repo(fresh.clone(), vec![])
        .await
        .unwrap();
    assert_eq!(empty.get_repo_root().await.unwrap(), fresh.cid);
    assert!(empty.all_intents().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_failed_transaction_leaves_no_trace() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    txn.create_repo(vec![], true).await.unwrap();
    let root_before = txn.get_repo_root().await.unwrap();
    // a record whose uri names another repository fails indexing inside the
    // transaction, after the root and blocks were written
    let mut foreign = post_write("3jt5vlkorbad1", "foreign");
    foreign.uri = "at://example.com/app.bsky.feed.post/3jt5vlkorbad1".to_owned();
    let err = txn
        .process_writes(vec![PreparedWrite::Create(foreign)], None)
        .await;
    assert!(err.is_err());
    assert_eq!(txn.get_repo_root().await.unwrap(), root_before);
    assert_eq!(txn.all_intents().await.unwrap().len(), 2);
    assert_eq!(txn.record.record_count().await.unwrap(), 0);
}

/// A blob promoted for a write whose transaction then fails stays
/// unreadable: the promotion is only recorded by the transaction, so the
/// blob is served once a record referencing it commits.
#[tokio::test]
async fn a_failed_write_leaves_its_blob_unreadable_until_a_write_commits() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let blobs = blobstore();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobs.clone())
        .await
        .unwrap();
    txn.create_repo(vec![], true).await.unwrap();
    let metadata = txn
        .blob
        .upload_blob_and_get_metadata("text/plain".to_owned(), b"pending".to_vec())
        .await
        .unwrap();
    let blob_ref = txn.blob.track_untethered_blob(metadata).await.unwrap();
    let cid = blob_ref.get_cid().unwrap();
    let prepared = rsky_repo::types::PreparedBlobRef {
        cid,
        mime_type: "text/plain".to_owned(),
        constraints: rsky_repo::types::BlobConstraint {
            max_size: None,
            accept: None,
        },
    };
    let mut foreign = post_write("3jt5vlkorbad2", "with blob");
    foreign.uri = "at://example.com/app.bsky.feed.post/3jt5vlkorbad2".to_owned();
    foreign.blobs = vec![prepared.clone()];
    assert!(txn
        .process_writes(vec![PreparedWrite::Create(foreign)], None)
        .await
        .is_err());
    assert!(
        blobs.has_stored(cid).await.unwrap(),
        "promoted before the transaction"
    );
    assert!(txn.blob.get_blob_metadata(cid).await.is_err(), "not served");

    let mut good = post_write("3jt5vlkorgood", "with blob");
    good.blobs = vec![prepared];
    txn.process_writes(vec![PreparedWrite::Create(good)], None)
        .await
        .unwrap();
    assert!(txn.blob.get_blob_metadata(cid).await.is_ok());
}

const ALLOWLIST: &str = r#"
version = 1
default = "absent"

[entries]
"did:example:alice" = "active"
"did:example:draining" = "draining"
"#;

async fn admitted_store() -> (tempfile::TempDir, ActorStore) {
    let (dir, store) = test_store(10).await;
    let allowlist = dir.path().join("write-allowlist.toml");
    std::fs::write(&allowlist, ALLOWLIST).unwrap();
    let admission = Arc::new(crate::admission::Admission::from_file(&allowlist).unwrap());
    let lock_dir = crate::locks::LockDir::new(dir.path().join("locks")).unwrap();
    let store = store.with_admission(admission).with_lock_dir(lock_dir);
    (dir, store)
}

fn not_admitted(err: &anyhow::Error) -> Option<&crate::admission::NotAdmitted> {
    err.downcast_ref::<crate::admission::NotAdmitted>()
}

#[tokio::test]
async fn writes_follow_the_allowlist_and_hold_the_actor_lock() {
    let (dir, store) = admitted_store().await;
    let keypair = test_keypair();
    let refused = store
        .create("did:example:draining", &keypair)
        .await
        .unwrap_err();
    assert_eq!(not_admitted(&refused).unwrap().state, "draining");
    let refused = store
        .create("did:example:nobody", &keypair)
        .await
        .unwrap_err();
    assert_eq!(not_admitted(&refused).unwrap().state, "absent");
    store.create(TEST_DID, &keypair).await.unwrap();

    let locks = crate::locks::LockDir::new(dir.path().join("locks")).unwrap();
    assert_eq!(store.inflight_mutations(TEST_DID), 0);
    let txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    assert_eq!(store.inflight_mutations(TEST_DID), 1);
    assert!(locks.try_exclusive(TEST_DID).unwrap().is_none());
    drop(txn);
    assert_eq!(store.inflight_mutations(TEST_DID), 0);
    assert!(locks.try_exclusive(TEST_DID).unwrap().is_some());

    // a draining actor accepts no new write; an absent one nothing at all
    reference_store(&store, "did:example:draining").await;
    let refused = store
        .transact("did:example:draining".to_owned(), blobstore())
        .await
        .map(drop)
        .unwrap_err();
    assert_eq!(not_admitted(&refused).unwrap().state, "draining");
    reference_store(&store, "did:example:nobody").await;
    let refused = store.unlink("did:example:nobody").await.unwrap_err();
    assert_eq!(not_admitted(&refused).unwrap().state, "absent");
    let refused = store
        .delete_blobs("did:example:nobody", blobstore())
        .await
        .unwrap_err();
    assert_eq!(not_admitted(&refused).unwrap().state, "absent");
    store
        .lifecycle
        .mark_pending_work("did:example:nobody")
        .await
        .unwrap();
    store.queue_blob_work("did:example:nobody", blobstore());
    store.background_queue.process_all().await;
    assert_eq!(
        store.lifecycle.pending_work().await.unwrap(),
        ["did:example:nobody"]
    );
    // a worker still runs for a draining actor
    assert!(store.unlink("did:example:draining").await.is_ok());
}

/// A revision floor lifts every later commit above a boundary consumers
/// have already seen, and an empty write list is a valid recovery commit.
#[tokio::test]
async fn commits_rise_above_the_revision_floor() {
    let (_dir, store) = test_store(10).await;
    store.create(TEST_DID, &test_keypair()).await.unwrap();
    let mut txn = store
        .transact(TEST_DID.to_owned(), blobstore())
        .await
        .unwrap();
    txn.create_repo(vec![], true).await.unwrap();
    let floor = "3zzzzzzzzzzzz";
    store
        .lifecycle
        .raise_revision_floor(TEST_DID, floor)
        .await
        .unwrap();
    let recovery = txn.process_writes(vec![], None).await.unwrap();
    assert!(recovery.commit_data.rev.as_str() > floor);
    assert!(recovery.ops.is_empty());
    let next = txn
        .process_writes(
            vec![PreparedWrite::Create(post_write("3jt5vlkorflr1", "x"))],
            None,
        )
        .await
        .unwrap();
    assert!(next.commit_data.rev > recovery.commit_data.rev);
    assert_eq!(
        next.commit_data.since.as_deref(),
        Some(recovery.commit_data.rev.as_str())
    );
}
