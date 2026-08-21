use rsky_pds::actor_store::space::SpaceStore;
use rsky_space_host::repo::RepoStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDump {
    pub records: Vec<(String, String, String, Vec<u8>)>,
    pub lthash_state: Vec<u8>,
    pub ops: Vec<(String, String, Option<String>, Option<String>)>,
}

pub async fn dump_shim(store: &dyn RepoStore, space_uri: &str, did: &str) -> RepoDump {
    let (records, _) = store
        .list_records(space_uri, did, None, None, u32::MAX)
        .await
        .expect("shim records");
    let head = store
        .head(space_uri, did)
        .await
        .expect("shim head")
        .expect("shim repo");
    let ops = store
        .list_ops(space_uri, did, None, None, u32::MAX)
        .await
        .expect("shim ops");
    RepoDump {
        records: records
            .into_iter()
            .map(|r| (r.collection, r.rkey, r.cid, r.value))
            .collect(),
        lthash_state: head.state.to_vec(),
        ops: ops
            .ops
            .into_iter()
            .map(|o| (o.collection, o.rkey, o.cid, o.prev))
            .collect(),
    }
}

pub async fn dump_pds(store: &SpaceStore, space_uri: &str) -> RepoDump {
    let records = store.all_records(space_uri).await.expect("pds records");
    let state = store
        .live_repo_state(space_uri)
        .await
        .expect("pds repo state");
    let (ops, _) = store
        .list_repo_ops(space_uri, None, None, usize::MAX >> 1)
        .await
        .expect("pds ops");
    RepoDump {
        records: records
            .into_iter()
            .map(|r| (r.collection, r.rkey, r.cid, r.value))
            .collect(),
        lthash_state: state.lthash_state,
        ops: ops
            .into_iter()
            .map(|o| (o.collection, o.rkey, o.cid, o.prev))
            .collect(),
    }
}

pub fn assert_parity(name: &str, shim: &RepoDump, pds: &RepoDump) -> bool {
    let mut equal = true;
    for (field, left, right) in [
        (
            "records",
            format!("{:?}", shim.records),
            format!("{:?}", pds.records),
        ),
        (
            "lthash_state",
            format!("{:?}", shim.lthash_state),
            format!("{:?}", pds.lthash_state),
        ),
        ("ops", format!("{:?}", shim.ops), format!("{:?}", pds.ops)),
    ] {
        if left != right {
            eprintln!("{name}: {field} differs\n  shim: {left}\n  pds:  {right}");
            equal = false;
        }
    }
    equal
}
