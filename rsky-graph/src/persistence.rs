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

    rtxn.commit()
        .map_err(|e| GraphError::Other(format!("read commit failed: {e}")))?;

    // Rebuild bloom filters from loaded data
    if count > 0 {
        crate::bloom::build_all_bloom_filters(graph);
    }

    tracing::info!("loaded {count} users from LMDB, max_uid={max_uid}");
    Ok(count)
}

pub async fn save_to_lmdb(db_path: &str, graph: &FollowGraph) -> Result<(), GraphError> {
    let path = Path::new(db_path);
    if !path.exists() {
        std::fs::create_dir_all(path)
            .map_err(|e| GraphError::Other(format!("create dir failed: {e}")))?;
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

    // LMDB rejects empty keys with MDB_BAD_VALSIZE; one stray empty DID would abort the whole txn.
    let mut skipped_empty = 0u64;
    for entry in graph.did_to_uid.iter() {
        if entry.key().is_empty() {
            skipped_empty += 1;
            continue;
        }
        let uid_bytes = entry.value().to_ne_bytes();
        did_uid_db
            .put(&mut wtxn, entry.key(), &uid_bytes)
            .map_err(|e| GraphError::Other(format!("put did_uid failed: {e}")))?;
    }
    for entry in graph.uid_to_did.iter() {
        if entry.value().is_empty() {
            continue;
        }
        let did_bytes = entry.value().as_bytes();
        uid_did_db
            .put(&mut wtxn, entry.key(), did_bytes)
            .map_err(|e| GraphError::Other(format!("put uid_did failed: {e}")))?;
    }
    if skipped_empty > 0 {
        tracing::warn!("skipped {skipped_empty} empty-DID entries during persistence");
    }

    // Save following bitmaps
    for entry in graph.following.iter() {
        let mut buf = Vec::new();
        entry
            .value()
            .serialize_into(&mut buf)
            .map_err(|e| GraphError::Other(format!("serialize following failed: {e}")))?;
        following_db
            .put(&mut wtxn, entry.key(), &buf)
            .map_err(|e| GraphError::Other(format!("put following failed: {e}")))?;
    }

    // Save followers bitmaps
    for entry in graph.followers.iter() {
        let mut buf = Vec::new();
        entry
            .value()
            .serialize_into(&mut buf)
            .map_err(|e| GraphError::Other(format!("serialize followers failed: {e}")))?;
        followers_db
            .put(&mut wtxn, entry.key(), &buf)
            .map_err(|e| GraphError::Other(format!("put followers failed: {e}")))?;
    }

    wtxn.commit()
        .map_err(|e| GraphError::Other(format!("commit failed: {e}")))?;

    tracing::debug!("saved {} users to LMDB", graph.user_count());
    Ok(())
}
