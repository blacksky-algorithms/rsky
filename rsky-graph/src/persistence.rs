use crate::graph::FollowGraph;
use crate::types::GraphError;
use heed::types::*;
use heed::{Database, EnvOpenOptions};
use roaring::RoaringBitmap;
use std::io::Cursor;
use std::path::Path;

type StrDb = Database<Str, Bytes>;
type U32Db = Database<U32<heed::byteorder::LE>, Bytes>;

pub async fn load_from_lmdb(db_path: &str, graph: &FollowGraph) -> Result<usize, GraphError> {
    let path = Path::new(db_path);
    if !path.exists() {
        std::fs::create_dir_all(path)
            .map_err(|e| GraphError::Other(format!("create dir failed: {e}")))?;
        return Ok(0);
    }

    let env = unsafe {
        EnvOpenOptions::new()
            .map_size(100 * 1024 * 1024 * 1024) // 100GB max
            .max_dbs(4)
            .open(path)
            .map_err(|e| GraphError::Other(format!("lmdb open failed: {e}")))?
    };

    let mut wtxn = env
        .write_txn()
        .map_err(|e| GraphError::Other(format!("txn failed: {e}")))?;

    // DID -> UID mapping
    let did_uid_db: StrDb = env
        .create_database(&mut wtxn, Some("did_uid"))
        .map_err(|e| GraphError::Other(format!("create db failed: {e}")))?;

    // UID -> DID mapping
    let uid_did_db: U32Db = env
        .create_database(&mut wtxn, Some("uid_did"))
        .map_err(|e| GraphError::Other(format!("create db failed: {e}")))?;

    // Following bitmaps: UID -> serialized RoaringBitmap
    let following_db: U32Db = env
        .create_database(&mut wtxn, Some("following"))
        .map_err(|e| GraphError::Other(format!("create db failed: {e}")))?;

    // Followers bitmaps: UID -> serialized RoaringBitmap
    let followers_db: U32Db = env
        .create_database(&mut wtxn, Some("followers"))
        .map_err(|e| GraphError::Other(format!("create db failed: {e}")))?;

    wtxn.commit()
        .map_err(|e| GraphError::Other(format!("commit failed: {e}")))?;

    let rtxn = env
        .read_txn()
        .map_err(|e| GraphError::Other(format!("read txn failed: {e}")))?;

    // Load DID <-> UID mappings
    let mut max_uid: u32 = 0;
    let mut count = 0usize;

    let iter = did_uid_db
        .iter(&rtxn)
        .map_err(|e| GraphError::Other(format!("iter failed: {e}")))?;

    for result in iter {
        let (did, uid_bytes) =
            result.map_err(|e| GraphError::Other(format!("iter read failed: {e}")))?;
        if uid_bytes.len() < 4 {
            continue;
        }
        let uid = u32::from_ne_bytes(uid_bytes[..4].try_into().unwrap());
        graph.did_to_uid.insert(did.to_owned(), uid);
        graph.uid_to_did.insert(uid, did.to_owned());
        if uid > max_uid {
            max_uid = uid;
        }
        count += 1;
    }

    graph.set_next_uid(max_uid + 1);

    // Load following bitmaps
    let iter = following_db
        .iter(&rtxn)
        .map_err(|e| GraphError::Other(format!("following iter failed: {e}")))?;

    for result in iter {
        let (uid, bytes) =
            result.map_err(|e| GraphError::Other(format!("following read failed: {e}")))?;
        let bm = RoaringBitmap::deserialize_from(Cursor::new(bytes))
            .map_err(|e| GraphError::Other(format!("bitmap deser failed: {e}")))?;
        graph.following.insert(uid, bm);
    }

    // Load followers bitmaps
    let iter = followers_db
        .iter(&rtxn)
        .map_err(|e| GraphError::Other(format!("followers iter failed: {e}")))?;

    for result in iter {
        let (uid, bytes) =
            result.map_err(|e| GraphError::Other(format!("followers read failed: {e}")))?;
        let bm = RoaringBitmap::deserialize_from(Cursor::new(bytes))
            .map_err(|e| GraphError::Other(format!("bitmap deser failed: {e}")))?;
        graph.followers.insert(uid, bm);
    }

    // Restore follow_count from the bitmap state (load doesn't go through add_follow).
    let total: u64 = graph.following.iter().map(|e| e.value().len()).sum();
    graph.set_follow_count(total);

    rtxn.commit()
        .map_err(|e| GraphError::Other(format!("read commit failed: {e}")))?;

    // Bloom filters are NOT rebuilt eagerly: at 25M+ users this is a
    // multi-hour single-threaded CPU loop (each Bloom::new calls getrandom
    // twice, plus 1.4B set() ops) that blocks api::serve and the bulk-load
    // resume. blooms are repopulated incrementally as add_follow is called
    // by firehose + bulk-load; absent blooms only forfeit the fast-reject
    // optimization, the bitmap intersection still returns the correct answer.
    tracing::info!("loaded {count} users from LMDB, max_uid={max_uid}");
    Ok(count)
}

/// Incremental save: only persists users in `graph.dirty_users` since the
/// last successful save. The full O(N) snapshot is only used when the dirty
/// set is empty AND LMDB is missing -- e.g. cold start from an empty data
/// dir. After load_from_lmdb, dirty_users is empty and steady-state mutations
/// re-mark it via add_follow/remove_follow, so per-save IO scales with
/// recent write volume, not total graph size.
pub async fn save_to_lmdb(db_path: &str, graph: &FollowGraph) -> Result<(), GraphError> {
    let path = Path::new(db_path);
    let cold_start = !path.join("data.mdb").exists();
    if !path.exists() {
        std::fs::create_dir_all(path)
            .map_err(|e| GraphError::Other(format!("create dir failed: {e}")))?;
    }

    let dirty: Vec<u32> = graph.drain_dirty_users();
    if dirty.is_empty() && !cold_start {
        // Nothing changed since the last commit and the on-disk snapshot
        // already exists. Skip opening LMDB entirely.
        tracing::debug!("save: clean, no entries to persist");
        return Ok(());
    }

    let env = unsafe {
        EnvOpenOptions::new()
            .map_size(100 * 1024 * 1024 * 1024)
            .max_dbs(4)
            .open(path)
            .map_err(|e| GraphError::Other(format!("lmdb open failed: {e}")))?
    };

    let mut wtxn = env
        .write_txn()
        .map_err(|e| GraphError::Other(format!("txn failed: {e}")))?;

    let did_uid_db: StrDb = env
        .create_database(&mut wtxn, Some("did_uid"))
        .map_err(|e| GraphError::Other(format!("create db failed: {e}")))?;
    let uid_did_db: U32Db = env
        .create_database(&mut wtxn, Some("uid_did"))
        .map_err(|e| GraphError::Other(format!("create db failed: {e}")))?;
    let following_db: U32Db = env
        .create_database(&mut wtxn, Some("following"))
        .map_err(|e| GraphError::Other(format!("create db failed: {e}")))?;
    let followers_db: U32Db = env
        .create_database(&mut wtxn, Some("followers"))
        .map_err(|e| GraphError::Other(format!("create db failed: {e}")))?;

    if cold_start && dirty.is_empty() {
        // First-ever save with no in-memory mutations: snapshot everything
        // already in the DashMaps so the on-disk file becomes a valid
        // starting point for later incremental saves.
        save_full(
            graph,
            &mut wtxn,
            &did_uid_db,
            &uid_did_db,
            &following_db,
            &followers_db,
        )?;
    } else {
        save_dirty(
            graph,
            &dirty,
            &mut wtxn,
            &did_uid_db,
            &uid_did_db,
            &following_db,
            &followers_db,
        )?;
    }

    wtxn.commit()
        .map_err(|e| GraphError::Other(format!("commit failed: {e}")))?;

    tracing::debug!(
        "saved {} dirty entries to LMDB ({} users total)",
        dirty.len(),
        graph.user_count()
    );
    Ok(())
}

/// Write all entries in the graph -- used only on cold start (no data.mdb yet)
/// or by an explicit full-snapshot path. Linear in graph size.
fn save_full(
    graph: &FollowGraph,
    wtxn: &mut heed::RwTxn<'_>,
    did_uid_db: &StrDb,
    uid_did_db: &U32Db,
    following_db: &U32Db,
    followers_db: &U32Db,
) -> Result<(), GraphError> {
    let mut skipped_empty = 0u64;
    for entry in graph.did_to_uid.iter() {
        if entry.key().is_empty() {
            skipped_empty += 1;
            continue;
        }
        let uid_bytes = entry.value().to_ne_bytes();
        did_uid_db
            .put(wtxn, entry.key(), &uid_bytes)
            .map_err(|e| GraphError::Other(format!("put did_uid failed: {e}")))?;
    }
    for entry in graph.uid_to_did.iter() {
        if entry.value().is_empty() {
            continue;
        }
        let did_bytes = entry.value().as_bytes();
        uid_did_db
            .put(wtxn, entry.key(), did_bytes)
            .map_err(|e| GraphError::Other(format!("put uid_did failed: {e}")))?;
    }
    if skipped_empty > 0 {
        tracing::warn!("skipped {skipped_empty} empty-DID entries during persistence");
    }
    for entry in graph.following.iter() {
        let mut buf = Vec::new();
        entry
            .value()
            .serialize_into(&mut buf)
            .map_err(|e| GraphError::Other(format!("serialize following failed: {e}")))?;
        following_db
            .put(wtxn, entry.key(), &buf)
            .map_err(|e| GraphError::Other(format!("put following failed: {e}")))?;
    }
    for entry in graph.followers.iter() {
        let mut buf = Vec::new();
        entry
            .value()
            .serialize_into(&mut buf)
            .map_err(|e| GraphError::Other(format!("serialize followers failed: {e}")))?;
        followers_db
            .put(wtxn, entry.key(), &buf)
            .map_err(|e| GraphError::Other(format!("put followers failed: {e}")))?;
    }
    Ok(())
}

/// Write only the entries belonging to `dirty` UIDs. Linear in `dirty.len()`,
/// independent of total graph size.
fn save_dirty(
    graph: &FollowGraph,
    dirty: &[u32],
    wtxn: &mut heed::RwTxn<'_>,
    did_uid_db: &StrDb,
    uid_did_db: &U32Db,
    following_db: &U32Db,
    followers_db: &U32Db,
) -> Result<(), GraphError> {
    let mut skipped_empty = 0u64;
    for uid in dirty {
        // Persist DID <-> UID for this user. uid_to_did is the source-of-truth
        // for "what DID does this UID map to"; did_to_uid is its inverse.
        if let Some(did_ref) = graph.uid_to_did.get(uid) {
            let did = did_ref.value();
            if did.is_empty() {
                skipped_empty += 1;
            } else {
                did_uid_db
                    .put(wtxn, did, &uid.to_ne_bytes())
                    .map_err(|e| GraphError::Other(format!("put did_uid failed: {e}")))?;
                uid_did_db
                    .put(wtxn, uid, did.as_bytes())
                    .map_err(|e| GraphError::Other(format!("put uid_did failed: {e}")))?;
            }
        }
        // Following bitmap. Read-locks this DashMap shard; concurrent writes
        // to the same UID block briefly. If the concurrent write arrives
        // *after* this read, drain_dirty_users()'s pre-remove guarantees the
        // UID is re-marked, so the next save catches up.
        if let Some(bm_ref) = graph.following.get(uid) {
            let mut buf = Vec::new();
            bm_ref
                .value()
                .serialize_into(&mut buf)
                .map_err(|e| GraphError::Other(format!("serialize following failed: {e}")))?;
            following_db
                .put(wtxn, uid, &buf)
                .map_err(|e| GraphError::Other(format!("put following failed: {e}")))?;
        }
        if let Some(bm_ref) = graph.followers.get(uid) {
            let mut buf = Vec::new();
            bm_ref
                .value()
                .serialize_into(&mut buf)
                .map_err(|e| GraphError::Other(format!("serialize followers failed: {e}")))?;
            followers_db
                .put(wtxn, uid, &buf)
                .map_err(|e| GraphError::Other(format!("put followers failed: {e}")))?;
        }
    }
    if skipped_empty > 0 {
        tracing::warn!("skipped {skipped_empty} empty-DID entries during persistence");
    }
    Ok(())
}
