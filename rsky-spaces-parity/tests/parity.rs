use oracle_rsky_space::space_id::SpaceId;
use rsky_pds::actor_store::db::get_migrated_db;
use rsky_pds::actor_store::space::{encode_record, oplog_window, SpaceStore, SpaceWrite};
use rsky_space_host::actor_repos::ActorStoreRepos;
use rsky_space_host::repo::{RepoStore, RepoWrite};
use rsky_spaces_parity::{assert_parity, dump_pds, dump_shim, pds_outcome, shim_outcome};
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

#[derive(Clone)]
enum ScriptWrite {
    Create {
        rkey: &'static str,
        value: Value,
    },
    Update {
        rkey: &'static str,
        value: Value,
        swap: Option<String>,
    },
    Delete {
        rkey: &'static str,
        swap: Option<String>,
    },
}

impl ScriptWrite {
    fn shim(&self) -> RepoWrite {
        match self {
            Self::Create { rkey, value } => RepoWrite::Create {
                collection: COLLECTION.into(),
                rkey: (*rkey).into(),
                value: encode_record(value).expect("record encoding").1,
            },
            Self::Update { rkey, value, swap } => RepoWrite::Update {
                collection: COLLECTION.into(),
                rkey: (*rkey).into(),
                value: encode_record(value).expect("record encoding").1,
                swap_record: swap.clone(),
            },
            Self::Delete { rkey, swap } => RepoWrite::Delete {
                collection: COLLECTION.into(),
                rkey: (*rkey).into(),
                swap_record: swap.clone(),
            },
        }
    }

    fn pds(&self) -> SpaceWrite {
        match self {
            Self::Create { rkey, value } => SpaceWrite::Create {
                collection: COLLECTION.into(),
                rkey: (*rkey).into(),
                value: value.clone(),
            },
            Self::Update { rkey, value, swap } => SpaceWrite::Update {
                collection: COLLECTION.into(),
                rkey: (*rkey).into(),
                value: value.clone(),
                swap_cid: swap.clone(),
            },
            Self::Delete { rkey, swap } => SpaceWrite::Delete {
                collection: COLLECTION.into(),
                rkey: (*rkey).into(),
                swap_cid: swap.clone(),
            },
        }
    }
}

fn create(rkey: &'static str, text: &str) -> ScriptWrite {
    ScriptWrite::Create {
        rkey,
        value: json!({ "text": text }),
    }
}

fn cid_of(value: &Value) -> String {
    encode_record(value).expect("record encoding").0
}

/// Drive both stores through the same batches, comparing the outcome of each
/// batch and then the two stores' contents.
async fn run(name: &str, batches: Vec<Vec<ScriptWrite>>) -> bool {
    let space = SpaceId::new(AUTHORITY, "community.blacksky.feed", "parity");
    let space_uri = space.uri();
    let temp = tempfile::tempdir().expect("tempdir");
    let shim = ActorStoreRepos::open(temp.path().join("shim")).expect("shim store");
    let pds = SpaceStore::new(
        DID.into(),
        get_migrated_db(temp.path().join("store.sqlite"))
            .await
            .expect("pds db"),
    );

    for (index, batch) in batches.iter().enumerate() {
        let shim_result = shim
            .apply_writes(
                &space_uri,
                DID,
                "3shimrev",
                &batch.iter().map(ScriptWrite::shim).collect::<Vec<_>>(),
            )
            .await;
        let pds_result = pds
            .apply_writes(
                &space,
                batch.iter().map(ScriptWrite::pds).collect(),
                oplog_window(),
            )
            .await;
        let (shim_outcome, pds_outcome) = (shim_outcome(&shim_result), pds_outcome(&pds_result));
        if shim_outcome != pds_outcome {
            eprintln!(
                "{name}: batch {index} outcomes differ\n  shim: {shim_outcome:?}\n  pds:  {pds_outcome:?}"
            );
            return false;
        }
    }

    assert_parity(
        name,
        &dump_shim(&shim, &space_uri, DID).await,
        &dump_pds(&pds, &space_uri).await,
    )
}

fn scenarios() -> Vec<(&'static str, Vec<Vec<ScriptWrite>>)> {
    let first = json!({ "text": "first" });
    vec![
        (
            "S1 create single record",
            vec![vec![create("one", "first")]],
        ),
        (
            "S2 batch create",
            vec![vec![
                create("one", "one"),
                create("two", "two"),
                create("three", "three"),
            ]],
        ),
        (
            "S3 update with swap-cid success",
            vec![
                vec![create("one", "first")],
                vec![ScriptWrite::Update {
                    rkey: "one",
                    value: json!({ "text": "second" }),
                    swap: Some(cid_of(&first)),
                }],
            ],
        ),
        (
            "S4 swap-cid conflict",
            vec![
                vec![create("one", "first")],
                vec![ScriptWrite::Update {
                    rkey: "one",
                    value: json!({ "text": "second" }),
                    swap: Some(cid_of(&json!({ "text": "stale" }))),
                }],
            ],
        ),
        (
            "S5 delete",
            vec![
                vec![create("one", "first"), create("two", "two")],
                vec![ScriptWrite::Delete {
                    rkey: "one",
                    swap: None,
                }],
            ],
        ),
        (
            "S6 delete then recreate same rkey",
            vec![
                vec![create("one", "first")],
                vec![ScriptWrite::Delete {
                    rkey: "one",
                    swap: Some(cid_of(&first)),
                }],
                vec![create("one", "reborn")],
                vec![ScriptWrite::Delete {
                    rkey: "one",
                    swap: None,
                }],
                vec![ScriptWrite::Delete {
                    rkey: "one",
                    swap: None,
                }],
            ],
        ),
    ]
}

#[tokio::test]
async fn scoreboard() {
    let scenarios = scenarios();
    let total = scenarios.len();
    let mut equal = 0;
    for (name, batches) in scenarios {
        equal += usize::from(run(name, batches).await);
    }
    println!("parity: {equal}/{total} scenarios byte-equal");
    assert_eq!(equal, total, "every scenario must be byte-equal");
}
