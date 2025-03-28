use crate::carstore::{CarStoreError, CarStoreResult, CompactionTarget, UserViewSource};
use chrono::prelude::*;
use lexicon_cid::Cid;
use rsky_common::env::env_str;
use rsky_common::models::Uid;
use rsky_repo::cid_set::CidSet;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Error as SurrealError;
use surrealdb::Surreal;

pub struct CarStoreSurreal {
    pub meta: Surreal<Db>,
}

#[derive(Debug, Deserialize)]
struct BlockRefInfo {
    path: String,
    offset: i64,
    usr: Uid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StaleRef {
    pub id: Option<String>,
    pub cid: Option<Cid>,
    pub cids: Vec<u8>,
    pub usr: Uid,
}

impl StaleRef {
    pub fn get_cids(&self) -> Result<Vec<Cid>, Box<dyn Error>> {
        match &self.cid {
            Some(cid) => Ok(vec![cid.clone()]),
            None => unpack_cids(&self.cids),
        }
    }
}

// Unpack multiple CIDs from a contiguous byte array
fn unpack_cids(bytes: &[u8]) -> Result<Vec<Cid>, Box<dyn Error>> {
    let mut cids = Vec::new();
    let mut cursor = 0;

    while cursor < bytes.len() {
        let cid = Cid::try_from(&bytes[cursor..])?;
        let len = cid.encoded_len();
        cids.push(cid);
        cursor += len;
    }

    Ok(cids)
}

// Pack multiple CIDs into a contiguous byte array
fn pack_cids(cids: &[Cid]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(cids.iter().map(Cid::encoded_len).sum());
    for cid in cids {
        buf.extend_from_slice(cid.to_bytes().as_ref());
    }
    buf
}

impl UserViewSource for CarStoreSurreal {
    fn has_uid_cid<'a>(
        &'a self,
        user: &'a Uid,
        k: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, CarStoreError>> + Send + Sync + 'a>> {
        // Convert to owned values first
        let user_str = user.to_string();
        let cid_str = k.to_string();

        Box::pin(async move {
            let mut response = self
                .meta
                .query("SELECT count() AS count FROM blockRef WHERE usr = $user AND cid = $cid")
                .bind(("user", user_str))
                .bind(("cid", cid_str))
                .await?;

            let count: Option<i64> = response.take("count")?;
            Ok(count.unwrap_or(0) > 0)
        })
    }

    fn lookup_block_ref<'a>(
        &'a self,
        k: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<(String, i64, Uid), CarStoreError>> + Send + Sync + 'a>>
    {
        let cid_str = k.to_string();

        Box::pin(async move {
            let mut response = self
                .meta
                .query(
                    r#"
                SELECT
                    (SELECT path FROM car_shards WHERE id = block_refs.shard) AS path,
                    offset,
                    (SELECT usr FROM car_shards WHERE id = block_refs.shard) AS usr
                FROM block_refs
                WHERE cid = $cid
                LIMIT 1
            "#,
                )
                .bind(("cid", cid_str))
                .await?;

            let info: Option<BlockRefInfo> = response.take(0)?;

            match info {
                Some(i) => Ok((i.path, i.offset, i.usr)),
                None => Err(CarStoreError::SurrealDbError(SurrealError::Api(
                    surrealdb::error::Api::Query("No block ref found".to_string()),
                ))),
            }
        })
    }
}

impl CarStoreSurreal {
    pub async fn new() -> Result<Self, SurrealError> {
        let db_path = env_str("CARSTORE_DB_PATH").unwrap_or(String::from("data/surrealdb"));
        let db = Surreal::new::<RocksDb>(db_path).await?;

        db.use_ns("cs").use_db("cs").await?;
        Ok(Self { meta: db })
    }

    pub async fn init(&self) -> Result<(), SurrealError> {
        // Define our tables. Although SurrealDB is schemaless, these statements
        // clarify the intended structure.
        self.meta.query("DEFINE TABLE CarShard SCHEMALESS;").await?;
        self.meta.query("DEFINE TABLE blockRef SCHEMALESS;").await?;
        self.meta
            .query(
                "
            DEFINE TABLE staleRef SCHEMALESS;
            DEFINE INDEX idx_stale_ref_usr ON staleRef FIELDS usr;
        ",
            )
            .await?;
        Ok(())
    }

    pub async fn lookup_block_ref(&self, cid: &Cid) -> Result<(String, i64, Uid), SurrealError> {
        let cid_str = cid.to_string();

        let mut response = self
            .meta
            .query(
                r#"
                SELECT
                    (SELECT path FROM car_shards WHERE id = block_refs.shard) AS path,
                    offset,
                    (SELECT usr FROM car_shards WHERE id = block_refs.shard) AS usr
                FROM block_refs
                WHERE cid = $cid
                LIMIT 1
            "#,
            )
            .bind(("cid", cid_str))
            .await?;

        let info: Option<BlockRefInfo> = response.take(0)?;

        match info {
            Some(i) => Ok((i.path, i.offset, i.usr)),
            None => Err(SurrealError::Api(surrealdb::error::Api::Query(
                "No block ref found".to_string(),
            ))),
        }
    }

    pub async fn get_last_shard(&self, user: &Uid) -> Result<Option<CarShard>, SurrealError> {
        let user_str = user.to_string();

        let mut response = self
            .meta
            .query(
                "
                SELECT * FROM CarShard
                WHERE usr = $user
                ORDER BY seq DESC
                LIMIT 1
            ",
            )
            .bind(("user", user_str))
            .await?;

        let shard: Option<CarShard> = response.take(0)?;
        Ok(shard)
    }

    pub async fn get_user_shards(&self, user: &Uid) -> Result<Vec<CarShard>, SurrealError> {
        let user_str = user.to_string();

        let mut response = self
            .meta
            .query(
                "
                SELECT * FROM CarShard
                WHERE usr = $user
                ORDER BY seq ASC
            ",
            )
            .bind(("user", user_str))
            .await?;

        let shards: Vec<CarShard> = response.take(0)?;
        Ok(shards)
    }

    pub async fn get_user_shards_desc(
        &self,
        user: &Uid,
        min_seq: i32,
    ) -> Result<Vec<CarShard>, SurrealError> {
        let user_str = user.to_string();

        let mut response = self
            .meta
            .query(
                "
                SELECT * FROM CarShard
                WHERE usr = $user AND seq >= $min_seq
                ORDER BY seq DESC
            ",
            )
            .bind(("user", user_str))
            .bind(("min_seq", min_seq))
            .await?;

        let shards: Vec<CarShard> = response.take(0)?;
        Ok(shards)
    }

    pub async fn get_user_stale_refs(&self, user: &Uid) -> Result<Vec<StaleRef>, SurrealError> {
        let user_str = user.to_string();

        let mut response = self
            .meta
            .query("SELECT * FROM staleRef WHERE usr = $user")
            .bind(("user", user_str))
            .await?;

        let refs: Vec<StaleRef> = response.take(0)?;
        Ok(refs)
    }

    /// Return the `seq` of the earliest shard whose revision >= since_rev and user = user
    pub async fn seq_for_rev(&self, user: &Uid, since_rev: &str) -> CarStoreResult<i64> {
        let user_str = user.to_string();
        let rev_str = since_rev.to_string();

        let sql = r#"
            SELECT * FROM car_shards
            WHERE rev >= $rev
              AND usr = $usr
            ORDER BY rev ASC
            LIMIT 1
        "#;

        let mut res = self
            .meta
            .query(sql)
            .bind(("rev", rev_str))
            .bind(("usr", user_str))
            .await?; // SurrealError -> CarStoreError via From

        // Take the first (and only) result set
        let shards: Vec<CarShard> = res.take(0)?;

        match shards.into_iter().next() {
            Some(shard) => Ok(shard.seq),
            None => Err(CarStoreError::NotFound(format!(
                "No shard found for user={} rev>={}",
                user, since_rev
            ))),
        }
    }

    pub async fn get_compaction_targets(
        &self,
        min_shard_count: i64,
    ) -> CarStoreResult<Vec<CompactionTarget>> {
        let sql = r#"
            SELECT usr, count() as num_shards
            FROM car_shards
            GROUP BY usr
        "#;

        let mut res = self.meta.query(sql).await?;
        let mut raw: Vec<CompactionTarget> = res.take(0)?;

        // Filter out those with <= min_shard_count
        raw.retain(|t| t.num_shards > min_shard_count);

        // Sort descending by num_shards
        raw.sort_by_key(|t| -t.num_shards);

        Ok(raw)
    }

    /// Create a shard + the associated block refs in a transaction.
    /// If `rmcids` is non-empty, creates a `StaleRef` as well.
    pub async fn put_shard_and_refs(
        &self,
        shard: CarShard,
        mut brefs: Vec<HashMap<String, JsonValue>>,
        rmcids: &CidSet,
    ) -> CarStoreResult<()> {
        // Begin transaction
        self.meta.query("BEGIN TRANSACTION").await?;

        // 1. Create the CarShard
        let create_shard_sql = r#"
            CREATE type::table($table) CONTENT $shard
        "#;
        let table_name = "car_shards";

        let mut shard_res = self
            .meta
            .query(create_shard_sql)
            .bind(("table", table_name))
            .bind(("shard", shard))
            .await?;

        let created_shards: Vec<CarShard> = shard_res.take(0)?;
        let created_shard = match created_shards.into_iter().next() {
            Some(s) => s,
            None => {
                // Cancel transaction if creation fails
                self.meta.query("CANCEL TRANSACTION").await?;
                return Err(CarStoreError::Error(
                    "Failed to create shard in SurrealDB".to_string(),
                ));
            }
        };

        let shard_id = match &created_shard.id {
            Some(id) => id.clone(),
            None => {
                self.meta.query("CANCEL TRANSACTION").await?;
                return Err(CarStoreError::Error(
                    "No shard ID returned from SurrealDB".to_string(),
                ));
            }
        };

        // 2. Prepare block refs
        for bref in &mut brefs {
            bref.insert("shard".to_string(), JsonValue::String(shard_id.clone()));
        }

        // 3. Create block_refs in bulk (if any), but in batches
        if !brefs.is_empty() {
            create_block_refs_in_batches(&self.meta, &brefs, 2000).await?;
        }

        // 4. If we have rmcids, create a staleRef
        if rmcids.size() > 0 {
            let cids_vec: Vec<Cid> = rmcids.to_list().into_iter().collect();
            let packed = pack_cids(&cids_vec);

            let stale = StaleRef {
                id: None,
                cid: None, // if your schema includes a single "cid" field
                cids: packed,
                usr: created_shard.usr.clone(),
            };

            let sql_stale = "CREATE staleRefs CONTENT $stale";
            self.meta.query(sql_stale).bind(("stale", stale)).await?;
        }

        // 5. Commit
        self.meta.query("COMMIT TRANSACTION").await?;

        Ok(())
    }

    /// Delete shards and associated block_refs by shard ID
    pub async fn delete_shards_and_refs(&self, ids: &[String]) -> CarStoreResult<()> {
        let ids_vec = ids.to_vec();
        // Begin
        self.meta.query("BEGIN TRANSACTION").await?;

        // Delete shards
        let sql_delete_shards = r#"
            DELETE FROM car_shards
            WHERE id IN $shard_ids
        "#;
        self.meta
            .query(sql_delete_shards)
            .bind(("shard_ids", ids_vec.clone()))
            .await?;

        // Delete refs
        let sql_delete_refs = r#"
            DELETE FROM block_refs
            WHERE shard IN $shard_ids
        "#;
        self.meta
            .query(sql_delete_refs)
            .bind(("shard_ids", ids_vec))
            .await?;

        // Commit
        self.meta.query("COMMIT TRANSACTION").await?;
        Ok(())
    }

    /// Get block_refs for a list of shard IDs (in chunks to avoid large `IN` lists).
    pub async fn get_block_refs_for_shards(
        &self,
        shard_ids: &[String],
    ) -> CarStoreResult<Vec<BlockRef>> {
        let chunk_size = 2000;
        let mut out = Vec::new();
        let mut idx = 0;

        while idx < shard_ids.len() {
            let end = (idx + chunk_size).min(shard_ids.len());
            let chunk = &shard_ids[idx..end];

            let sql = r#"
                SELECT *
                FROM block_refs
                WHERE shard IN $chunk
            "#;

            let mut res = self.meta.query(sql).bind(("chunk", chunk.to_vec())).await?;
            let mut fetched: Vec<BlockRef> = res.take(0)?;
            out.append(&mut fetched);

            idx += chunk_size;
        }

        Ok(out)
    }

    /// Fetch all block references for the given shard IDs.
    pub async fn block_refs_for_shards(
        &self,
        shard_ids: &[String],
    ) -> CarStoreResult<Vec<BlockRef>> {
        let sql = r#"
            SELECT * 
            FROM block_refs 
            WHERE shard IN $shard_ids
        "#;

        let mut response = self
            .meta
            .query(sql)
            .bind(("shard_ids", shard_ids.to_vec()))
            .await?;

        // SurrealDB can parse the result into your Vec<BlockRef> structure
        let block_refs: Vec<BlockRef> = response.take(0)?;
        Ok(block_refs)
    }

    /// Set the stale references for a given user.
    /// - Delete all existing staleRef entries for `uid`
    /// - Optionally create a new staleRef with the CIDs in `stale_to_keep`
    pub async fn set_stale_ref(&self, uid: &Uid, stale_to_keep: &[Cid]) -> CarStoreResult<()> {
        self.meta.query("BEGIN TRANSACTION").await?;

        let sql_delete = r#"
            DELETE FROM staleRefs
            WHERE usr = $uid
        "#;
        self.meta
            .query(sql_delete)
            .bind(("uid", uid.to_string()))
            .await?;

        if !stale_to_keep.is_empty() {
            let packed_cids = pack_cids(stale_to_keep);
            let new_stale = StaleRef {
                id: None,
                usr: *uid,
                cids: packed_cids,
                cid: None,
            };

            // "CREATE staleRefs CONTENT $data"
            let sql_insert = r#"
                CREATE staleRefs CONTENT $data
            "#;

            self.meta
                .query(sql_insert)
                .bind(("data", new_stale))
                .await?;
        }

        self.meta.query("COMMIT TRANSACTION").await?;

        Ok(())
    }
}

pub async fn create_block_refs_in_batches(
    db: &Surreal<Db>,
    brefs: &[HashMap<String, JsonValue>],
    batch_size: usize,
) -> Result<(), CarStoreError> {
    let mut i = 0;
    while i < brefs.len() {
        let end = (i + batch_size).min(brefs.len());
        let chunk = &brefs[i..end];

        let sql = "CREATE block_refs CONTENT $chunk";
        db.query(sql).bind(("chunk", chunk.to_vec())).await?;

        i = end;
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CarShard {
    pub id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub root: Cid,
    pub data_start: i64,
    pub seq: i64,
    pub path: String,
    pub usr: Uid,
    pub rev: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockRef {
    pub id: Option<String>,
    pub cid: Cid,
    pub shard: Option<String>,
    pub offset: i64,
}
