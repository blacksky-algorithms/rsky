use oracle_rsky_space::space_id::SpaceId;
use rsky_pds::actor_store::db::get_migrated_db;
use rsky_pds::actor_store::space::{
    blob_refs_in_record, encode_record, SpaceStore, SpaceWrite, DEFAULT_OPLOG_WINDOW,
};
use rsky_space_host::actor_repos::ActorStoreRepos;
use rsky_space_host::repo::{RepoStore, RepoWrite};
use rsky_spaces_parity::{
    assert_parity, compare_tables, dump_pds, dump_shim, dump_tables, pds_outcome,
    revs_are_well_formed, shim_outcome,
};
use serde_json::{json, Value};

fn sqlite_master(path: &std::path::Path) -> Vec<(String, String, String)> {
    let connection = rusqlite::Connection::open(path).expect("schema connection");
    let mut statement = connection
        .prepare("SELECT type, name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name")
        .expect("schema statement");
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("schema rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("schema values")
}

#[tokio::test]
async fn actor_schema_matches_pinned_oracle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let shim_path = temp.path().join("shim.sqlite");
    let pds_path = temp.path().join("pds.sqlite");
    rsky_space_host::actor_schema::get_migrated_db(&shim_path).expect("shim migration");
    get_migrated_db(&pds_path).await.expect("pds migration");
    assert_eq!(sqlite_master(&shim_path), sqlite_master(&pds_path));
}

const DID: &str = "did:plc:parityauthor";
const AUTHORITY: &str = "did:plc:parityauthority";
const COLLECTION: &str = "app.bsky.feed.post";
const PAGE: u32 = 2;

#[derive(Clone)]
struct ScriptWrite {
    space: usize,
    collection: &'static str,
    rkey: String,
    action: Action,
}

#[derive(Clone)]
enum Action {
    Create(Value),
    Update(Value, Option<String>),
    Delete(Option<String>),
}

impl ScriptWrite {
    fn shim(&self) -> RepoWrite {
        let (collection, rkey) = (self.collection.to_string(), self.rkey.clone());
        match &self.action {
            Action::Create(value) => RepoWrite::Create {
                collection,
                rkey,
                value: encoded(value),
            },
            Action::Update(value, swap) => RepoWrite::Update {
                collection,
                rkey,
                value: encoded(value),
                swap_record: swap.clone(),
            },
            Action::Delete(swap) => RepoWrite::Delete {
                collection,
                rkey,
                swap_record: swap.clone(),
            },
        }
    }

    fn pds(&self) -> SpaceWrite {
        let (collection, rkey) = (self.collection.to_string(), self.rkey.clone());
        match &self.action {
            Action::Create(value) => SpaceWrite::Create {
                collection,
                rkey,
                value: value.clone(),
            },
            Action::Update(value, swap) => SpaceWrite::Update {
                collection,
                rkey,
                value: value.clone(),
                swap_cid: swap.clone(),
            },
            Action::Delete(swap) => SpaceWrite::Delete {
                collection,
                rkey,
                swap_cid: swap.clone(),
            },
        }
    }
}

fn encoded(value: &Value) -> Vec<u8> {
    encode_record(value).expect("record encoding").1
}

fn cid_of(value: &Value) -> String {
    encode_record(value).expect("record encoding").0
}

fn create(rkey: &str, text: &str) -> ScriptWrite {
    ScriptWrite {
        space: 0,
        collection: COLLECTION,
        rkey: rkey.to_string(),
        action: Action::Create(json!({ "text": text })),
    }
}

fn update(rkey: &str, text: &str, swap: Option<String>) -> ScriptWrite {
    ScriptWrite {
        space: 0,
        collection: COLLECTION,
        rkey: rkey.to_string(),
        action: Action::Update(json!({ "text": text }), swap),
    }
}

fn delete(rkey: &str, swap: Option<String>) -> ScriptWrite {
    ScriptWrite {
        space: 0,
        collection: COLLECTION,
        rkey: rkey.to_string(),
        action: Action::Delete(swap),
    }
}

fn in_space(space: usize, write: ScriptWrite) -> ScriptWrite {
    ScriptWrite { space, ..write }
}

fn in_collection(collection: &'static str, write: ScriptWrite) -> ScriptWrite {
    ScriptWrite {
        collection,
        ..write
    }
}

struct Scenario {
    name: &'static str,
    window: usize,
    batches: Vec<Vec<ScriptWrite>>,
}

fn scenario(name: &'static str, batches: Vec<Vec<ScriptWrite>>) -> Scenario {
    Scenario {
        name,
        window: DEFAULT_OPLOG_WINDOW,
        batches,
    }
}

/// Drive both stores through the same batches, then compare: refusal reasons
/// per batch, the rows each side stored (with server-minted revisions and oplog
/// ids normalized), the reads each side serves whole and paged, the answers to
/// every `since` probe, and finally what the oracle reads back out of the
/// file the shim wrote.
async fn run(case: &Scenario) -> bool {
    let name = case.name;
    let spaces = [
        SpaceId::new(AUTHORITY, "community.blacksky.feed", "parity"),
        SpaceId::new(AUTHORITY, "community.blacksky.feed", "second"),
    ];
    let temp = tempfile::tempdir().expect("tempdir");
    let shim = ActorStoreRepos::with_oplog_window(temp.path().join("shim"), case.window)
        .expect("shim store");
    let pds_path = temp.path().join("store.sqlite");
    let pds = SpaceStore::new(
        DID.into(),
        get_migrated_db(&pds_path).await.expect("pds db"),
    );

    for (index, batch) in case.batches.iter().enumerate() {
        for (space_index, space) in spaces.iter().enumerate() {
            let writes: Vec<&ScriptWrite> =
                batch.iter().filter(|w| w.space == space_index).collect();
            if writes.is_empty() {
                continue;
            }
            let shim_result = shim
                .apply_writes(
                    &space.uri(),
                    DID,
                    "3shimrev",
                    &writes.iter().map(|w| w.shim()).collect::<Vec<_>>(),
                )
                .await;
            let pds_result = pds
                .apply_writes(space, writes.iter().map(|w| w.pds()).collect(), case.window)
                .await;
            let (shim_kind, pds_kind) = (shim_outcome(&shim_result), pds_outcome(&pds_result));
            if shim_kind != pds_kind {
                eprintln!(
                    "{name}: batch {index} space {space_index} outcomes differ\n  \
                     shim: {shim_kind:?}\n  pds:  {pds_kind:?}"
                );
                return false;
            }
        }
    }

    let shim_path = shim.store_path(DID).expect("shim store path");
    let (shim_tables, pds_tables) = (dump_tables(&shim_path), dump_tables(&pds_path));
    let mut equal = compare_tables(name, &shim_tables, &pds_tables)
        && revs_are_well_formed(name, &shim_tables, "shim")
        && revs_are_well_formed(name, &pds_tables, "pds");

    // The oracle, pointed at the shim's own file: the drop-in assertion.
    let crossed = SpaceStore::new(
        DID.into(),
        get_migrated_db(&shim_path).await.expect("cross-open db"),
    );

    for space in &spaces {
        let uri = space.uri();
        equal = equal
            && assert_parity(
                name,
                &dump_shim(&shim, &uri, DID).await,
                &dump_pds(&pds, &uri).await,
            )
            && assert_parity(
                &format!("{name} cross-open"),
                &dump_shim(&shim, &uri, DID).await,
                &dump_pds(&crossed, &uri).await,
            )
            && paged_reads_equal(name, &shim, &pds, &uri).await
            && since_reads_equal(name, &shim, &pds, &uri, &shim_tables.revs, &pds_tables.revs)
                .await;
    }
    equal
}

/// Walk both sides in `PAGE`-sized pages, comparing every page and the cursor
/// each side hands back.
async fn paged_reads_equal(
    name: &str,
    shim: &ActorStoreRepos,
    pds: &SpaceStore,
    space_uri: &str,
) -> bool {
    if shim
        .head(space_uri, DID)
        .await
        .expect("shim head")
        .is_none()
    {
        return true;
    }

    let mut cursor: Option<String> = None;
    loop {
        let (page, next) = shim
            .list_records(space_uri, DID, None, cursor.as_deref(), PAGE)
            .await
            .expect("shim record page");
        let mirror = pds
            .list_records(space_uri, None, PAGE as usize, cursor.clone())
            .await
            .expect("pds record page");
        let left: Vec<_> = page
            .iter()
            .map(|r| (&r.collection, &r.rkey, &r.cid, &r.value))
            .collect();
        let right: Vec<_> = mirror
            .iter()
            .map(|r| (&r.collection, &r.rkey, &r.cid, &r.value))
            .collect();
        if left != right {
            eprintln!(
                "{name}: record page after {cursor:?} differs\n  shim: {left:?}\n  pds:  {right:?}"
            );
            return false;
        }
        match next {
            Some(next) if !page.is_empty() => cursor = Some(next),
            _ => break,
        }
    }

    let mut cursor: Option<i64> = None;
    loop {
        let page = shim
            .list_ops(
                space_uri,
                DID,
                None,
                cursor.map(|c| c.to_string()).as_deref(),
                PAGE,
            )
            .await;
        let mirror = pds
            .list_repo_ops(space_uri, None, cursor, PAGE as usize)
            .await;
        let (shim_kind, pds_kind) = (shim_outcome(&page), pds_outcome(&mirror));
        if shim_kind != pds_kind {
            eprintln!(
                "{name}: op page after {cursor:?} outcomes differ\n  \
                 shim: {shim_kind:?}\n  pds:  {pds_kind:?}"
            );
            return false;
        }
        let (Ok(page), Ok((mirror, more))) = (page, mirror) else {
            return true;
        };
        let left: Vec<_> = page
            .ops
            .iter()
            .map(|o| (&o.collection, &o.rkey, &o.cid, &o.prev))
            .collect();
        let right: Vec<_> = mirror
            .iter()
            .map(|o| (&o.collection, &o.rkey, &o.cid, &o.prev))
            .collect();
        if left != right || page.complete == more {
            eprintln!(
                "{name}: op page after {cursor:?} differs\n  \
                 shim: {left:?} complete={}\n  pds:  {right:?} has_more={more}",
                page.complete
            );
            return false;
        }
        if !more {
            break;
        }
        cursor = mirror.last().map(|o| o.id);
    }
    true
}

/// Ask both sides for history since each revision they minted. The revisions
/// differ between the two stores, so each side is probed with its own — the
/// nth revision on one side answers the nth on the other.
async fn since_reads_equal(
    name: &str,
    shim: &ActorStoreRepos,
    pds: &SpaceStore,
    space_uri: &str,
    shim_revs: &[String],
    pds_revs: &[String],
) -> bool {
    for (index, (shim_rev, pds_rev)) in shim_revs.iter().zip(pds_revs).enumerate() {
        let page = shim
            .list_ops(space_uri, DID, Some(shim_rev), None, u32::MAX)
            .await;
        let mirror = pds
            .list_repo_ops(space_uri, Some(pds_rev.clone()), None, usize::MAX >> 1)
            .await;
        let (shim_kind, pds_kind) = (shim_outcome(&page), pds_outcome(&mirror));
        if shim_kind != pds_kind {
            eprintln!(
                "{name}: history since revision {index} differs\n  \
                 shim: {shim_kind:?}\n  pds:  {pds_kind:?}"
            );
            return false;
        }
        let (Ok(page), Ok((mirror, _))) = (page, mirror) else {
            continue;
        };
        let left: Vec<_> = page
            .ops
            .iter()
            .map(|o| (&o.collection, &o.rkey, &o.cid, &o.prev))
            .collect();
        let right: Vec<_> = mirror
            .iter()
            .map(|o| (&o.collection, &o.rkey, &o.cid, &o.prev))
            .collect();
        if left != right {
            eprintln!(
                "{name}: history since revision {index} differs\n  \
                 shim: {left:?}\n  pds:  {right:?}"
            );
            return false;
        }
    }
    true
}

fn scenarios() -> Vec<Scenario> {
    let first = json!({ "text": "first" });
    let long = "x".repeat(60 * 1024);
    vec![
        scenario(
            "S1 create single record",
            vec![vec![create("one", "first")]],
        ),
        scenario(
            "S2 batch create",
            vec![vec![
                create("one", "one"),
                create("two", "two"),
                create("three", "three"),
            ]],
        ),
        scenario(
            "S3 update with swap-cid success",
            vec![
                vec![create("one", "first")],
                vec![update("one", "second", Some(cid_of(&first)))],
            ],
        ),
        scenario(
            "S4 swap-cid conflict",
            vec![
                vec![create("one", "first")],
                vec![update(
                    "one",
                    "second",
                    Some(cid_of(&json!({ "text": "stale" }))),
                )],
            ],
        ),
        scenario(
            "S5 delete",
            vec![
                vec![create("one", "first"), create("two", "two")],
                vec![delete("one", None)],
            ],
        ),
        scenario(
            "S6 delete then recreate same rkey",
            vec![
                vec![create("one", "first")],
                vec![delete("one", Some(cid_of(&first)))],
                vec![create("one", "reborn")],
                vec![delete("one", None)],
                vec![delete("one", None)],
            ],
        ),
        scenario(
            "S7 unicode and prefix-colliding keys",
            vec![vec![
                create("é🌍", "unicode rkey"),
                create("a-b", "hyphen sorts before slash"),
                create("a", "bare"),
                create("a.b", "dotted"),
                in_collection("app.bsky.feed.pos", create("z", "shorter collection")),
                in_collection("app.bsky.feed.post-x", create("a", "longer collection")),
                in_collection("app.bsky.feed.post.deep", create("a", "deeper collection")),
            ]],
        ),
        scenario(
            "S8 large record",
            vec![
                vec![create("big", &long)],
                vec![update(
                    "big",
                    "small again",
                    Some(cid_of(&json!({ "text": long }))),
                )],
            ],
        ),
        Scenario {
            name: "S9 oplog compaction beyond the window",
            window: 3,
            batches: (0..6)
                .map(|i| vec![create(&format!("r{i}"), &format!("body {i}"))])
                .chain(std::iter::once(vec![
                    delete("r0", None),
                    delete("r1", None),
                ]))
                .collect(),
        },
        scenario(
            "S10 two spaces one author",
            vec![
                vec![
                    create("one", "in first space"),
                    in_space(1, create("one", "in second space")),
                ],
                vec![
                    in_space(1, create("two", "second space only")),
                    delete("one", None),
                ],
            ],
        ),
        scenario(
            "S11 pagination across many revisions",
            (0..7)
                .map(|i| vec![create(&format!("r{i}"), &format!("body {i}"))])
                .chain(std::iter::once(vec![
                    delete("r3", None),
                    update("r4", "edited", None),
                ]))
                .collect(),
        ),
        scenario(
            "S13 cross-open after a mixed script",
            vec![
                vec![create("keep", "kept"), create("gone", "removed")],
                vec![
                    update("keep", "edited", Some(cid_of(&json!({ "text": "kept" })))),
                    delete("gone", None),
                    in_collection("app.bsky.feed.like", create("l1", "liked")),
                ],
                vec![in_space(1, create("elsewhere", "other space"))],
            ],
        ),
    ]
}

/// S12: blobs are the one documented divergence. The oracle accepts a
/// blob-bearing record and indexes the reference; the shim's storage keeps the
/// bytes but indexes nothing, and the host's write path refuses the record
/// outright, so a blob-carrying space cannot use the drop-in path.
#[tokio::test]
async fn s12_blob_bearing_record_is_a_documented_divergence() {
    let space = SpaceId::new(AUTHORITY, "community.blacksky.feed", "parity");
    let temp = tempfile::tempdir().expect("tempdir");
    let shim = ActorStoreRepos::open(temp.path().join("shim")).expect("shim store");
    let pds = SpaceStore::new(
        DID.into(),
        get_migrated_db(temp.path().join("store.sqlite"))
            .await
            .expect("pds db"),
    );
    let record = json!({
        "text": "with an image",
        "image": {
            "$type": "blob",
            "ref": { "$link": "bafkreibme22gw2h7y2h7tg2fhqotaqjucnbc24deqo72b6mkl2egm4gv4a" },
            "mimeType": "image/png",
            "size": 12345,
        },
    });
    assert_eq!(blob_refs_in_record(&record).len(), 1);
    assert!(rsky_space_host::http::contains_blob_ref(&record));

    pds.apply_writes(
        &space,
        vec![SpaceWrite::Create {
            collection: COLLECTION.into(),
            rkey: "one".into(),
            value: record.clone(),
        }],
        DEFAULT_OPLOG_WINDOW,
    )
    .await
    .expect("pds accepts blob-bearing records");
    shim.apply_writes(
        &space.uri(),
        DID,
        "3shimrev",
        &[RepoWrite::Create {
            collection: COLLECTION.into(),
            rkey: "one".into(),
            value: encoded(&record),
        }],
    )
    .await
    .expect("shim storage is shape-agnostic");

    let shim_blobs = blob_refs(&shim.store_path(DID).expect("shim store path"));
    let pds_blobs = blob_refs(&temp.path().join("store.sqlite"));
    assert_eq!(pds_blobs, 1, "oracle indexes the blob reference");
    assert_eq!(shim_blobs, 0, "shim indexes no blob references");
}

fn blob_refs(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .expect("store connection")
        .query_row("SELECT COUNT(*) FROM space_blob_ref", [], |row| row.get(0))
        .expect("blob ref count")
}

#[tokio::test]
async fn scoreboard() {
    let scenarios = scenarios();
    let total = scenarios.len();
    let mut equal = 0;
    for case in &scenarios {
        equal += usize::from(run(case).await);
    }
    println!("parity: {equal}/{total} (+1 documented divergence) scenarios byte-equal");
    assert_eq!(equal, total, "every scenario must be byte-equal");
}
