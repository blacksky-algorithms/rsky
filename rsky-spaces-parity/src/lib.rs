use rsky_pds::actor_store::space::{SpaceStore, SpaceStoreError};
use rsky_space_host::error::HostError;
use rsky_space_host::repo::RepoStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDump {
    pub records: Vec<(String, String, String, Vec<u8>)>,
    pub lthash_state: Vec<u8>,
    pub ops: Vec<(String, String, Option<String>, Option<String>)>,
}

/// The classification a write batch or read is compared on: either it applied,
/// or both sides must refuse it for the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    RecordExists,
    RecordNotFound,
    InvalidSwap,
    HistoryUnavailable,
    RepoGone,
    Other(String),
}

pub fn shim_outcome<T>(result: &Result<T, HostError>) -> Outcome {
    match result {
        Ok(_) => Outcome::Applied,
        Err(HostError::RecordExists(_)) => Outcome::RecordExists,
        Err(HostError::RecordNotFound(_)) => Outcome::RecordNotFound,
        Err(HostError::InvalidSwap) => Outcome::InvalidSwap,
        Err(HostError::HistoryUnavailable) => Outcome::HistoryUnavailable,
        Err(HostError::RepoNotFound) => Outcome::RepoGone,
        Err(other) => Outcome::Other(other.to_string()),
    }
}

pub fn pds_outcome<T>(result: &anyhow::Result<T>) -> Outcome {
    let Err(error) = result else {
        return Outcome::Applied;
    };
    match error.downcast_ref::<SpaceStoreError>() {
        Some(SpaceStoreError::RecordExists(_)) => Outcome::RecordExists,
        Some(SpaceStoreError::RecordNotFound(_)) => Outcome::RecordNotFound,
        Some(SpaceStoreError::InvalidSwap(_)) => Outcome::InvalidSwap,
        Some(SpaceStoreError::HistoryUnavailable) => Outcome::HistoryUnavailable,
        Some(SpaceStoreError::SpaceNotFound(_)) | Some(SpaceStoreError::SpaceDeleted(_)) => {
            Outcome::RepoGone
        }
        None => Outcome::Other(error.to_string()),
    }
}

/// `None` when the repo does not exist, so a scenario whose first batch is
/// rejected on both sides still compares.
pub async fn dump_shim(store: &dyn RepoStore, space_uri: &str, did: &str) -> Option<RepoDump> {
    let head = store.head(space_uri, did).await.expect("shim head")?;
    let (records, _) = store
        .list_records(space_uri, did, None, None, u32::MAX)
        .await
        .expect("shim records");
    let ops = store
        .list_ops(space_uri, did, None, None, u32::MAX)
        .await
        .expect("shim ops");
    Some(RepoDump {
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
    })
}

pub async fn dump_pds(store: &SpaceStore, space_uri: &str) -> Option<RepoDump> {
    let state = store.repo_state(space_uri).await.expect("pds repo state")?;
    if state.deleted {
        return None;
    }
    let records = store.all_records(space_uri).await.expect("pds records");
    let (ops, _) = store
        .list_repo_ops(space_uri, None, None, usize::MAX >> 1)
        .await
        .expect("pds ops");
    Some(RepoDump {
        records: records
            .into_iter()
            .map(|r| (r.collection, r.rkey, r.cid, r.value))
            .collect(),
        lthash_state: state.lthash_state,
        ops: ops
            .into_iter()
            .map(|o| (o.collection, o.rkey, o.cid, o.prev))
            .collect(),
    })
}

pub fn assert_parity(name: &str, shim: &Option<RepoDump>, pds: &Option<RepoDump>) -> bool {
    match (shim, pds) {
        (None, None) => true,
        (Some(shim), Some(pds)) => compare(name, shim, pds),
        (shim, pds) => {
            eprintln!(
                "{name}: repo existence differs: shim={}, pds={}",
                shim.is_some(),
                pds.is_some()
            );
            false
        }
    }
}

fn compare(name: &str, shim: &RepoDump, pds: &RepoDump) -> bool {
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
