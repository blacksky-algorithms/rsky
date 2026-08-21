use oracle_rsky_space::space_id::SpaceId;
use rsky_pds::actor_store::db::get_migrated_db;
use rsky_pds::actor_store::space::{encode_record, oplog_window, SpaceStore, SpaceWrite};
use rsky_space_host::repo::{ActorStoreRepos, RepoStore, RepoWrite};
use rsky_spaces_parity::{assert_parity, dump_pds, dump_shim};
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

async fn run(name: &str, script: Vec<ScriptWrite>) -> bool {
    let space = SpaceId::new(AUTHORITY, "community.blacksky.feed", "parity");
    let space_uri = space.uri();
    let temp = tempfile::tempdir().expect("tempdir");
    let shim = ActorStoreRepos::open(temp.path()).expect("shim store");
    let pds = SpaceStore::new(
        DID.into(),
        get_migrated_db(temp.path().join("store.sqlite"))
            .await
            .expect("pds db"),
    );
    let shim_result = shim
        .apply_writes(
            &space_uri,
            DID,
            "3shimrev",
            &script.iter().map(ScriptWrite::shim).collect::<Vec<_>>(),
        )
        .await;
    let pds_result = pds
        .apply_writes(
            &space,
            script.iter().map(ScriptWrite::pds).collect(),
            oplog_window(),
        )
        .await;
    let equal_outcome = match (shim_result, pds_result) {
        (Ok(_), Ok(_)) => assert_parity(
            name,
            &dump_shim(&shim, &space_uri, DID).await,
            &dump_pds(&pds, &space_uri).await,
        ),
        (Err(_), Err(_)) => false,
        (left, right) => {
            eprintln!(
                "{name}: outcomes differ: shim_ok={}, pds_ok={}",
                left.is_ok(),
                right.is_ok()
            );
            false
        }
    };
    equal_outcome
}

#[tokio::test]
async fn scoreboard() {
    let first = json!({"text": "first"});
    let first_cid = encode_record(&first).expect("record encoding").0;
    let scenarios = [
        (
            "S1 create",
            vec![ScriptWrite::Create {
                rkey: "one",
                value: first.clone(),
            }],
        ),
        (
            "S2 batch create",
            vec![
                ScriptWrite::Create {
                    rkey: "one",
                    value: json!({"text":"one"}),
                },
                ScriptWrite::Create {
                    rkey: "two",
                    value: json!({"text":"two"}),
                },
                ScriptWrite::Create {
                    rkey: "three",
                    value: json!({"text":"three"}),
                },
            ],
        ),
        (
            "S3 update swap success",
            vec![
                ScriptWrite::Create {
                    rkey: "one",
                    value: first.clone(),
                },
                ScriptWrite::Update {
                    rkey: "one",
                    value: json!({"text":"second"}),
                    swap: Some(first_cid.clone()),
                },
            ],
        ),
        (
            "S4 swap conflict",
            vec![
                ScriptWrite::Create {
                    rkey: "one",
                    value: first,
                },
                ScriptWrite::Update {
                    rkey: "one",
                    value: json!({"text":"second"}),
                    swap: Some("bafyreinvalid".into()),
                },
            ],
        ),
        (
            "S5 delete",
            vec![
                ScriptWrite::Create {
                    rkey: "one",
                    value: json!({"text":"first"}),
                },
                ScriptWrite::Delete {
                    rkey: "one",
                    swap: None,
                },
            ],
        ),
    ];
    let mut equal = 0;
    for (name, script) in scenarios {
        equal += usize::from(run(name, script).await);
    }
    println!("parity: {equal}/5 scenarios byte-equal");
    assert_eq!(equal, 5, "parity harness must be red before convergence");
}
