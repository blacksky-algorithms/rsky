use super::*;
use crate::actor_store::db::get_migrated_db;
use serde_json::json;

const DID: &str = "did:plc:author";
const AUTHORITY: &str = "did:plc:auth";
const SPACE_TYPE: &str = "com.example.forum";
const SKEY: &str = "self";

async fn store() -> (tempfile::TempDir, SpaceStore) {
    let dir = tempfile::tempdir().unwrap();
    let db = get_migrated_db(dir.path().join("store.sqlite"))
        .await
        .unwrap();
    (dir, SpaceStore::new(DID.to_string(), db))
}

fn space() -> SpaceId {
    SpaceId::new(AUTHORITY, SPACE_TYPE, SKEY)
}

fn create(rkey: &str, text: &str) -> SpaceWrite {
    SpaceWrite::Create {
        collection: "com.example.post".to_string(),
        rkey: rkey.to_string(),
        value: json!({"text": text}),
    }
}

fn update(rkey: &str, text: &str) -> SpaceWrite {
    SpaceWrite::Update {
        collection: "com.example.post".to_string(),
        rkey: rkey.to_string(),
        value: json!({"text": text}),
        swap_cid: None,
    }
}

fn delete(rkey: &str) -> SpaceWrite {
    SpaceWrite::Delete {
        collection: "com.example.post".to_string(),
        rkey: rkey.to_string(),
        swap_cid: None,
    }
}

/// LtHash over the current record rows, recomputed from scratch.
async fn from_scratch_hash(store: &SpaceStore, space_uri: &str) -> [u8; 32] {
    let mut lt = LtHash::new();
    for row in store.all_records(space_uri).await.unwrap() {
        lt.add(&element(&row.collection, &row.rkey, &row.cid));
    }
    lt.hash()
}

#[tokio::test]
async fn write_read_roundtrip() {
    let (_dir, store) = store().await;
    let space = space();
    let commit = store
        .apply_writes(&space, vec![create("3ka", "hello")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    assert_eq!(commit.results.len(), 1);
    let cid = commit.results[0].cid.clone().unwrap();
    assert!(commit.results[0].prev.is_none());

    let record = store
        .get_record(&space.uri(), "com.example.post", "3ka")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.cid, cid);
    assert_eq!(record.rev, commit.rev);
    assert_eq!(
        decode_record(&record.value).unwrap(),
        json!({"text": "hello"})
    );

    // The stored CID matches an independent DAG-CBOR computation.
    let (expect_cid, expect_bytes) = encode_record(&json!({"text": "hello"})).unwrap();
    assert_eq!(record.cid, expect_cid);
    assert_eq!(record.value, expect_bytes);

    let state = store.live_repo_state(&space.uri()).await.unwrap();
    assert_eq!(state.rev, commit.rev);
    assert_eq!(state.authority, AUTHORITY);
    assert_eq!(state.hash(), commit.hash);

    assert!(store
        .get_record(&space.uri(), "com.example.post", "missing")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn lthash_matches_from_scratch_over_arbitrary_sequences() {
    let (_dir, store) = store().await;
    let space = space();
    let uri = space.uri();

    // create three, update one, delete one, re-create the deleted one
    store
        .apply_writes(
            &space,
            vec![create("a", "one"), create("b", "two"), create("c", "three")],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap();
    store
        .apply_writes(&space, vec![update("b", "two-v2")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    store
        .apply_writes(&space, vec![delete("a")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    let commit = store
        .apply_writes(&space, vec![create("a", "one-again")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();

    assert_eq!(commit.hash, from_scratch_hash(&store, &uri).await);

    // deleting everything returns the state to all-zeroes
    let commit = store
        .apply_writes(
            &space,
            vec![delete("a"), delete("b"), delete("c")],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap();
    assert_eq!(commit.hash, LtHash::new().hash());
    let state = store.live_repo_state(&uri).await.unwrap();
    assert_eq!(state.lthash_state, vec![0u8; 2048]);
}

#[tokio::test]
async fn delete_returns_hash_to_prior_state() {
    let (_dir, store) = store().await;
    let space = space();
    let before = store
        .apply_writes(&space, vec![create("a", "one")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    store
        .apply_writes(&space, vec![create("b", "two")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    let after = store
        .apply_writes(&space, vec![delete("b")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    assert_eq!(before.hash, after.hash);
}

#[tokio::test]
async fn strict_action_semantics() {
    let (_dir, store) = store().await;
    let space = space();
    store
        .apply_writes(&space, vec![create("a", "one")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();

    let err = store
        .apply_writes(&space, vec![create("a", "dup")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::RecordExists(_))
    ));

    let err = store
        .apply_writes(&space, vec![update("missing", "x")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::RecordNotFound(_))
    ));

    let err = store
        .apply_writes(&space, vec![delete("missing")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::RecordNotFound(_))
    ));

    // a failed batch rolls back entirely
    let err = store
        .apply_writes(
            &space,
            vec![create("b", "two"), create("a", "dup")],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("RecordExists"));
    assert!(store
        .get_record(&space.uri(), "com.example.post", "b")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn swap_cid_semantics() {
    let (_dir, store) = store().await;
    let space = space();
    let commit = store
        .apply_writes(&space, vec![create("a", "one")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    let cid = commit.results[0].cid.clone().unwrap();

    // matching swap succeeds
    store
        .apply_writes(
            &space,
            vec![SpaceWrite::Update {
                collection: "com.example.post".to_string(),
                rkey: "a".to_string(),
                value: json!({"text": "two"}),
                swap_cid: Some(cid.clone()),
            }],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap();

    // stale swap fails
    let err = store
        .apply_writes(
            &space,
            vec![SpaceWrite::Delete {
                collection: "com.example.post".to_string(),
                rkey: "a".to_string(),
                swap_cid: Some(cid),
            }],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::InvalidSwap(_))
    ));
}

#[tokio::test]
async fn oplog_records_batches_under_one_rev() {
    let (_dir, store) = store().await;
    let space = space();
    let uri = space.uri();
    let first = store
        .apply_writes(
            &space,
            vec![create("a", "one"), create("b", "two")],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap();
    let second = store
        .apply_writes(
            &space,
            vec![update("a", "one-v2"), delete("b")],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap();
    assert!(second.rev > first.rev);

    let (ops, has_more) = store.list_repo_ops(&uri, None, None, 10).await.unwrap();
    assert!(!has_more);
    assert_eq!(ops.len(), 4);
    assert_eq!(ops[0].rev, first.rev);
    assert_eq!(ops[1].rev, first.rev);
    assert_eq!(ops[2].rev, second.rev);
    assert_eq!(ops[3].rev, second.rev);
    // create: no prev; update: prev + new cid; delete: no cid
    assert!(ops[0].prev.is_none() && ops[0].cid.is_some());
    assert!(ops[2].prev.is_some() && ops[2].cid.is_some());
    assert!(ops[3].cid.is_none() && ops[3].prev.is_some());

    // stale values are not inlined: op[0]'s cid was replaced by the update
    assert!(ops[0].value.is_none());
    assert!(ops[2].value.is_some());
    assert!(ops[3].value.is_none());

    // since filters by rev
    let (ops, _) = store
        .list_repo_ops(&uri, Some(first.rev.clone()), None, 10)
        .await
        .unwrap();
    assert_eq!(ops.len(), 2);
    assert!(ops.iter().all(|op| op.rev == second.rev));

    // cursor pages within the result
    let (page, has_more) = store.list_repo_ops(&uri, None, None, 1).await.unwrap();
    assert!(has_more);
    assert_eq!(page.len(), 1);
    let (rest, has_more) = store
        .list_repo_ops(&uri, None, Some(page[0].id), 10)
        .await
        .unwrap();
    assert!(!has_more);
    assert_eq!(rest.len(), 3);
}

#[tokio::test]
async fn compaction_advances_floor_and_gates_history() {
    let (_dir, store) = store().await;
    let space = space();
    let uri = space.uri();
    let mut revs = Vec::new();
    for i in 0..6 {
        let commit = store
            .apply_writes(&space, vec![create(&format!("r{i}"), "x")], 3)
            .await
            .unwrap();
        revs.push(commit.rev);
    }
    let state = store.live_repo_state(&uri).await.unwrap();
    let floor = state.oplog_floor_rev.clone().unwrap();
    assert_eq!(floor, revs[2]);

    // full history is gone
    let err = store.list_repo_ops(&uri, None, None, 10).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::HistoryUnavailable)
    ));
    // since below the floor is gone
    let err = store
        .list_repo_ops(&uri, Some(revs[0].clone()), None, 10)
        .await
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::HistoryUnavailable)
    ));
    // since at the floor is servable
    let (ops, _) = store
        .list_repo_ops(&uri, Some(floor), None, 10)
        .await
        .unwrap();
    assert_eq!(ops.len(), 3);
}

#[tokio::test]
async fn list_records_pagination_and_filters() {
    let (_dir, store) = store().await;
    let space = space();
    let uri = space.uri();
    store
        .apply_writes(
            &space,
            vec![
                create("b", "two"),
                create("a", "one"),
                SpaceWrite::Create {
                    collection: "com.example.like".to_string(),
                    rkey: "z".to_string(),
                    value: json!({"subject": "at://x"}),
                },
            ],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap();

    let all = store.list_records(&uri, None, 10, None).await.unwrap();
    let paths: Vec<String> = all
        .iter()
        .map(|r| format!("{}/{}", r.collection, r.rkey))
        .collect();
    assert_eq!(
        paths,
        [
            "com.example.like/z",
            "com.example.post/a",
            "com.example.post/b"
        ]
    );

    let filtered = store
        .list_records(&uri, Some("com.example.post".to_string()), 10, None)
        .await
        .unwrap();
    assert_eq!(filtered.len(), 2);

    let page = store.list_records(&uri, None, 2, None).await.unwrap();
    assert_eq!(page.len(), 2);
    let cursor = format!("{}/{}", page[1].collection, page[1].rkey);
    let rest = store
        .list_records(&uri, None, 2, Some(cursor))
        .await
        .unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].rkey, "b");

    assert_eq!(store.all_records(&uri).await.unwrap().len(), 3);
}

#[tokio::test]
async fn blob_refs_follow_record_lifecycle() {
    let (_dir, store) = store().await;
    let space = space();
    let uri = space.uri();
    let typed = json!({
        "text": "with blob",
        "embed": {
            "$type": "blob",
            "ref": {"$link": "bafkreiblob1"},
            "mimeType": "image/png",
            "size": 100
        }
    });
    let untyped = json!({
        "text": "legacy",
        "media": [{"cid": "bafkreiblob2", "mimeType": "video/mp4"}]
    });
    assert_eq!(blob_cids_in_record(&typed), vec!["bafkreiblob1"]);
    assert_eq!(blob_cids_in_record(&untyped), vec!["bafkreiblob2"]);
    assert!(blob_cids_in_record(&json!({"text": "none"})).is_empty());
    assert!(blob_cids_in_record(&json!({"$type": "blob", "mimeType": "x"})).is_empty());

    store
        .apply_writes(
            &space,
            vec![SpaceWrite::Create {
                collection: "com.example.post".to_string(),
                rkey: "a".to_string(),
                value: typed,
            }],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap();
    assert!(store
        .space_references_blob(&uri, "bafkreiblob1")
        .await
        .unwrap());

    // update swaps the referenced blob
    store
        .apply_writes(
            &space,
            vec![SpaceWrite::Update {
                collection: "com.example.post".to_string(),
                rkey: "a".to_string(),
                value: untyped,
                swap_cid: None,
            }],
            DEFAULT_OPLOG_WINDOW,
        )
        .await
        .unwrap();
    assert!(!store
        .space_references_blob(&uri, "bafkreiblob1")
        .await
        .unwrap());
    assert!(store
        .space_references_blob(&uri, "bafkreiblob2")
        .await
        .unwrap());

    store
        .apply_writes(&space, vec![delete("a")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    assert!(!store
        .space_references_blob(&uri, "bafkreiblob2")
        .await
        .unwrap());
}

#[tokio::test]
async fn repo_lifecycle_flags_and_listing() {
    let (_dir, store) = store().await;
    let space = space();
    let uri = space.uri();

    assert!(store.repo_state(&uri).await.unwrap().is_none());
    let err = store.live_repo_state(&uri).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceNotFound(_))
    ));
    let err = store.list_repo_ops(&uri, None, None, 10).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceNotFound(_))
    ));

    store
        .apply_writes(&space, vec![create("a", "one")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();
    let other = SpaceId::new(AUTHORITY, SPACE_TYPE, "second");
    store
        .apply_writes(&other, vec![create("a", "one")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap();

    let spaces = store.list_spaces(10, None).await.unwrap();
    assert_eq!(spaces, vec![other.uri(), uri.clone()]);
    let page = store.list_spaces(1, None).await.unwrap();
    assert_eq!(page, vec![other.uri()]);
    let rest = store.list_spaces(10, Some(page[0].clone())).await.unwrap();
    assert_eq!(rest, vec![uri.clone()]);

    // deletion flags without erasing
    assert!(store.flag_repo_deleted(&uri).await.unwrap());
    assert!(!store
        .flag_repo_deleted("at://missing/space/x/y")
        .await
        .unwrap());
    let err = store.live_repo_state(&uri).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceDeleted(_))
    ));
    let err = store
        .apply_writes(&space, vec![create("b", "no")], DEFAULT_OPLOG_WINDOW)
        .await
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceDeleted(_))
    ));
    // record rows survive the flag
    assert!(store
        .get_record(&uri, "com.example.post", "a")
        .await
        .unwrap()
        .is_some());
    assert_eq!(
        store.list_spaces(10, None).await.unwrap(),
        vec![other.uri()]
    );
}

#[tokio::test]
async fn space_def_crud_and_members() {
    let (_dir, store) = store().await;
    let uri = space().uri();
    let def = SpaceDefRow {
        space_uri: uri.clone(),
        space_type: SPACE_TYPE.to_string(),
        skey: SKEY.to_string(),
        policy: "member-list".to_string(),
        app_access: "open".to_string(),
        allowed_clients: None,
        managing_app: None,
        deleted: false,
    };
    assert!(store.get_space_def(&uri).await.unwrap().is_none());
    let err = store.live_space_def(&uri).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceNotFound(_))
    ));

    store.create_space_def(def.clone()).await.unwrap();
    assert_eq!(store.live_space_def(&uri).await.unwrap(), def);

    // duplicate creation is rejected
    let err = store.create_space_def(def.clone()).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::RecordExists(_))
    ));

    let updated = SpaceDefRow {
        policy: "managing-app".to_string(),
        app_access: "allow-list".to_string(),
        allowed_clients: Some(vec!["https://app.example.com/client.json".to_string()]),
        managing_app: Some("did:web:app.example.com#managing_app".to_string()),
        ..def.clone()
    };
    store.update_space_def(updated.clone()).await.unwrap();
    assert_eq!(store.live_space_def(&uri).await.unwrap(), updated);

    let err = store
        .update_space_def(SpaceDefRow {
            space_uri: "at://missing/space/x/y".to_string(),
            ..updated.clone()
        })
        .await
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceNotFound(_))
    ));

    // members
    store.add_member(&uri, "did:plc:m2").await.unwrap();
    store.add_member(&uri, "did:plc:m1").await.unwrap();
    store.add_member(&uri, "did:plc:m1").await.unwrap(); // idempotent
    assert_eq!(
        store.list_members(&uri, 10, None).await.unwrap(),
        vec!["did:plc:m1", "did:plc:m2"]
    );
    let page = store.list_members(&uri, 1, None).await.unwrap();
    assert_eq!(page, vec!["did:plc:m1"]);
    assert_eq!(
        store
            .list_members(&uri, 10, Some(page[0].clone()))
            .await
            .unwrap(),
        vec!["did:plc:m2"]
    );
    store.remove_member(&uri, "did:plc:m1").await.unwrap();
    assert_eq!(
        store.list_members(&uri, 10, None).await.unwrap(),
        vec!["did:plc:m2"]
    );

    // deletion flags and blocks re-creation and updates
    assert!(store.flag_space_def_deleted(&uri).await.unwrap());
    assert!(!store.flag_space_def_deleted(&uri).await.unwrap());
    let err = store.live_space_def(&uri).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceDeleted(_))
    ));
    let err = store.create_space_def(def).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceDeleted(_))
    ));
    let err = store.update_space_def(updated).await.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<SpaceStoreError>(),
        Some(SpaceStoreError::SpaceNotFound(_))
    ));
}

#[tokio::test]
async fn writers_notify_registrations_and_jti() {
    let (_dir, store) = store().await;
    let uri = space().uri();

    store
        .upsert_writer(&uri, "did:plc:w1", "3ka", Some("aa".to_string()))
        .await
        .unwrap();
    store
        .upsert_writer(&uri, "did:plc:w1", "3kb", None)
        .await
        .unwrap();
    store
        .upsert_writer(&uri, "did:plc:w0", "3kc", None)
        .await
        .unwrap();
    let writers = store.list_writers(&uri, 10, None).await.unwrap();
    assert_eq!(writers.len(), 2);
    assert_eq!(writers[0].did, "did:plc:w0");
    assert_eq!(writers[1].rev, "3kb");
    assert!(writers[1].hash.is_none());
    let page = store.list_writers(&uri, 1, None).await.unwrap();
    let rest = store
        .list_writers(&uri, 10, Some(page[0].did.clone()))
        .await
        .unwrap();
    assert_eq!(rest.len(), 1);

    // repo + host notify registrations honor expiry and upsert
    store
        .register_repo_notify(&uri, "https://a.example", "2020-01-01T00:00:00.000Z")
        .await
        .unwrap();
    store
        .register_repo_notify(&uri, "https://b.example", "2999-01-01T00:00:00.000Z")
        .await
        .unwrap();
    store
        .register_repo_notify(&uri, "https://a.example", "2999-01-01T00:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(
        store
            .repo_notify_endpoints(&uri, &rsky_common::now())
            .await
            .unwrap(),
        vec!["https://a.example", "https://b.example"]
    );

    store
        .register_host_notify(&uri, "https://sync.example", "2999-01-01T00:00:00.000Z")
        .await
        .unwrap();
    store
        .register_host_notify(&uri, "https://old.example", "2020-01-01T00:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(
        store
            .host_notify_endpoints(&uri, &rsky_common::now())
            .await
            .unwrap(),
        vec!["https://sync.example"]
    );

    // jti: single-use, expired entries purged
    assert!(store.consume_jti("n1", 2000, 1000).await.unwrap());
    assert!(!store.consume_jti("n1", 2000, 1000).await.unwrap());
    assert!(store.consume_jti("n2", 9000, 3000).await.unwrap());
    // n1 expired at 2000 and was purged by the now=3000 call
    assert!(store.consume_jti("n1", 9000, 3000).await.unwrap());
}

#[tokio::test]
async fn oplog_window_env_override() {
    // not set: default
    std::env::remove_var("PDS_SPACE_OPLOG_WINDOW");
    assert_eq!(oplog_window(), DEFAULT_OPLOG_WINDOW);
    std::env::set_var("PDS_SPACE_OPLOG_WINDOW", "5");
    assert_eq!(oplog_window(), 5);
    std::env::set_var("PDS_SPACE_OPLOG_WINDOW", "0");
    assert_eq!(oplog_window(), 1);
    std::env::remove_var("PDS_SPACE_OPLOG_WINDOW");
}
