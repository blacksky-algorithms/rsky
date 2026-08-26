pub mod layer2;
pub mod resume;

use rsky_pds::actor_store::space::{SpaceStore, SpaceStoreError};
use rsky_space_host::error::HostError;
use rsky_space_host::repo::RepoStore;
use std::collections::BTreeMap;
use std::path::Path;

pub type OpTuple = (String, String, Option<String>, Option<String>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDump {
    pub records: Vec<(String, String, String, Vec<u8>)>,
    pub lthash_state: Vec<u8>,
    /// `Err` once compaction has dropped the start of history, which is itself
    /// a value both sides must agree on.
    pub ops: Result<Vec<OpTuple>, Outcome>,
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
    let ops = store.list_ops(space_uri, did, None, None, u32::MAX).await;
    Some(RepoDump {
        records: records
            .into_iter()
            .map(|r| (r.collection, r.rkey, r.cid, r.value))
            .collect(),
        lthash_state: head.state.to_vec(),
        ops: match ops {
            Ok(page) => Ok(page
                .ops
                .into_iter()
                .map(|o| (o.collection, o.rkey, o.cid, o.prev))
                .collect()),
            Err(error) => Err(shim_outcome::<()>(&Err(error))),
        },
    })
}

pub async fn dump_pds(store: &SpaceStore, space_uri: &str) -> Option<RepoDump> {
    let state = store.repo_state(space_uri).await.expect("pds repo state")?;
    if state.deleted {
        return None;
    }
    let records = store.all_records(space_uri).await.expect("pds records");
    let ops = store
        .list_repo_ops(space_uri, None, None, usize::MAX >> 1)
        .await;
    Some(RepoDump {
        records: records
            .into_iter()
            .map(|r| (r.collection, r.rkey, r.cid, r.value))
            .collect(),
        lthash_state: state.lthash_state,
        ops: match ops {
            Ok((page, _)) => Ok(page
                .into_iter()
                .map(|o| (o.collection, o.rkey, o.cid, o.prev))
                .collect()),
            Err(error) => Err(pds_outcome::<()>(&Err(error))),
        },
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

// ------------------------------------------------------- stored-row comparison

/// The `space_*` rows of one store file, rendered as strings with the values
/// that cannot match across two independent writers replaced by placeholders:
/// revisions (server-minted TIDs) by `R1, R2, …` in first-appearance order,
/// oplog row ids by `I1, I2, …`, and creation timestamps by `T`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDump {
    pub tables: Vec<(&'static str, Vec<String>)>,
    /// The distinct revisions in first-appearance order, unnormalized.
    pub revs: Vec<String>,
    /// Revisions in oplog order, unnormalized.
    pub oplog_revs: Vec<String>,
}

struct Normalizer {
    revs: BTreeMap<String, String>,
    order: Vec<String>,
    ids: BTreeMap<i64, String>,
}

impl Normalizer {
    fn rev(&mut self, rev: &str) -> String {
        if let Some(placeholder) = self.revs.get(rev) {
            return placeholder.clone();
        }
        let placeholder = format!("R{}", self.order.len() + 1);
        self.revs.insert(rev.to_string(), placeholder.clone());
        self.order.push(rev.to_string());
        placeholder
    }

    fn optional_rev(&mut self, rev: Option<String>) -> String {
        rev.map(|r| self.rev(&r)).unwrap_or_else(|| "-".to_string())
    }

    fn id(&mut self, id: i64) -> String {
        let next = format!("I{}", self.ids.len() + 1);
        self.ids.entry(id).or_insert(next).clone()
    }
}

pub fn dump_tables(path: &Path) -> TableDump {
    let conn = rusqlite::Connection::open(path).expect("store connection");
    let mut norm = Normalizer {
        revs: BTreeMap::new(),
        order: Vec::new(),
        ids: BTreeMap::new(),
    };

    let mut oplog_revs = Vec::new();
    let mut oplog = Vec::new();
    {
        let mut statement = conn
            .prepare(
                "SELECT id, space_uri, rev, collection, rkey, cid, prev \
                 FROM space_oplog ORDER BY id",
            )
            .expect("oplog statement");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })
            .expect("oplog rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("oplog values");
        for (id, space_uri, rev, collection, rkey, cid, prev) in rows {
            oplog_revs.push(rev.clone());
            oplog.push(format!(
                "{} {space_uri} {} {collection} {rkey} {} {}",
                norm.id(id),
                norm.rev(&rev),
                cid.unwrap_or_else(|| "-".into()),
                prev.unwrap_or_else(|| "-".into()),
            ));
        }
    }

    let mut records = Vec::new();
    {
        let mut statement = conn
            .prepare(
                "SELECT space_uri, collection, rkey, cid, rev, value \
                 FROM space_record ORDER BY space_uri, collection, rkey",
            )
            .expect("record statement");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                ))
            })
            .expect("record rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("record values");
        for (space_uri, collection, rkey, cid, rev, value) in rows {
            records.push(format!(
                "{space_uri} {collection} {rkey} {cid} {} {}",
                norm.rev(&rev),
                render(&value)
            ));
        }
    }

    let mut repos = Vec::new();
    {
        let mut statement = conn
            .prepare(
                "SELECT space_uri, authority, space_type, skey, rev, lthash_state, \
                        oplog_floor_rev, deleted FROM space_repo ORDER BY space_uri",
            )
            .expect("repo statement");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })
            .expect("repo rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("repo values");
        for (space_uri, authority, space_type, skey, rev, state, floor, deleted) in rows {
            repos.push(format!(
                "{space_uri} {authority} {space_type} {skey} {} {} {} {deleted}",
                norm.rev(&rev),
                render(&state),
                norm.optional_rev(floor),
            ));
        }
    }

    let blobs;
    {
        let mut statement = conn
            .prepare(
                "SELECT space_uri, blob_cid, collection, rkey FROM space_blob_ref \
                 ORDER BY space_uri, blob_cid, collection, rkey",
            )
            .expect("blob statement");
        blobs = statement
            .query_map([], |row| {
                Ok(format!(
                    "{} {} {} {}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?
                ))
            })
            .expect("blob rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("blob values");
    }

    TableDump {
        tables: vec![
            ("space_repo", repos),
            ("space_record", records),
            ("space_oplog", oplog),
            ("space_blob_ref", blobs),
        ],
        revs: norm.order,
        oplog_revs,
    }
}

pub fn compare_tables(name: &str, shim: &TableDump, pds: &TableDump) -> bool {
    let mut equal = true;
    for ((table, left), (_, right)) in shim.tables.iter().zip(pds.tables.iter()) {
        if left != right {
            eprintln!("{name}: {table} rows differ\n  shim: {left:?}\n  pds:  {right:?}");
            equal = false;
        }
    }
    equal
}

/// Every revision is a syntactically valid TID, and the oplog is ordered by
/// revision — the two properties the placeholders would otherwise hide.
pub fn revs_are_well_formed(name: &str, dump: &TableDump, side: &str) -> bool {
    let mut sound = true;
    for rev in &dump.revs {
        if !is_tid(rev) {
            eprintln!("{name}: {side} rev `{rev}` is not a valid TID");
            sound = false;
        }
    }
    for pair in dump.oplog_revs.windows(2) {
        if pair[0] > pair[1] {
            eprintln!(
                "{name}: {side} oplog order disagrees with rev order: `{}` then `{}`",
                pair[0], pair[1]
            );
            sound = false;
        }
    }
    sound
}

/// 13 characters of the sortable base32 alphabet, with the high bit of the
/// leading character clear.
pub fn is_tid(value: &str) -> bool {
    const ALPHABET: &str = "234567abcdefghijklmnopqrstuvwxyz";
    value.len() == 13
        && value.chars().all(|c| ALPHABET.contains(c))
        && value
            .chars()
            .next()
            .is_some_and(|c| "234567abcdefghij".contains(c))
}

fn render(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
