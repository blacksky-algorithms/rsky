// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/actor-store/db

use crate::db::migrator::{migrate_to_latest, Migration};
use crate::db::sqlite::Db;
use anyhow::Result;
use std::path::Path;

pub type ActorDb = Db;

pub const ACTOR_DB_MIGRATIONS: &[Migration] = &[Migration {
    name: "001",
    sql: "\
    CREATE TABLE repo_root (\
        did TEXT PRIMARY KEY, \
        cid TEXT NOT NULL, \
        rev TEXT NOT NULL, \
        \"indexedAt\" TEXT NOT NULL\
    );\
    CREATE TABLE repo_block (\
        cid TEXT PRIMARY KEY, \
        \"repoRev\" TEXT NOT NULL, \
        size INTEGER NOT NULL, \
        content BLOB NOT NULL\
    );\
    CREATE INDEX repo_block_repo_rev_idx ON repo_block (\"repoRev\", cid);\
    CREATE TABLE record (\
        uri TEXT PRIMARY KEY, \
        cid TEXT NOT NULL, \
        collection TEXT NOT NULL, \
        rkey TEXT NOT NULL, \
        \"repoRev\" TEXT NOT NULL, \
        \"indexedAt\" TEXT NOT NULL, \
        \"takedownRef\" TEXT\
    );\
    CREATE INDEX record_cid_idx ON record (cid);\
    CREATE INDEX record_collection_idx ON record (collection);\
    CREATE INDEX record_repo_rev_idx ON record (\"repoRev\");\
    CREATE TABLE blob (\
        cid TEXT PRIMARY KEY, \
        \"mimeType\" TEXT NOT NULL, \
        size INTEGER NOT NULL, \
        \"tempKey\" TEXT, \
        width INTEGER, \
        height INTEGER, \
        \"createdAt\" TEXT NOT NULL, \
        \"takedownRef\" TEXT\
    );\
    CREATE INDEX blob_tempkey_idx ON blob (\"tempKey\");\
    CREATE TABLE record_blob (\
        \"blobCid\" TEXT NOT NULL, \
        \"recordUri\" TEXT NOT NULL, \
        PRIMARY KEY (\"blobCid\", \"recordUri\")\
    );\
    CREATE TABLE backlink (\
        uri TEXT NOT NULL, \
        path TEXT NOT NULL, \
        \"linkTo\" TEXT NOT NULL, \
        PRIMARY KEY (uri, path)\
    );\
    CREATE INDEX backlink_link_to_idx ON backlink (path, \"linkTo\");\
    CREATE TABLE account_pref (\
        id INTEGER PRIMARY KEY AUTOINCREMENT, \
        name TEXT NOT NULL, \
        \"valueJson\" TEXT NOT NULL\
    );",
}];

pub fn get_db(location: impl AsRef<Path>) -> Result<ActorDb> {
    Db::open(location)
}

pub async fn get_migrated_db(location: impl AsRef<Path>) -> Result<ActorDb> {
    let db = get_db(location)?;
    migrate_to_latest(&db, ACTOR_DB_MIGRATIONS).await?;
    Ok(db)
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepoRoot {
    pub did: String,
    pub cid: String,
    pub rev: String,
    pub indexed_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepoBlock {
    pub cid: String,
    pub repo_rev: String,
    pub size: i64,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub uri: String,
    pub cid: String,
    pub collection: String,
    pub rkey: String,
    pub repo_rev: String,
    pub indexed_at: String,
    pub takedown_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Blob {
    pub cid: String,
    pub mime_type: String,
    pub size: i64,
    pub temp_key: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: String,
    pub takedown_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordBlob {
    pub blob_cid: String,
    pub record_uri: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Backlink {
    pub uri: String,
    pub path: String,
    pub link_to: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountPref {
    pub id: i64,
    pub name: String,
    pub value_json: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrates_actor_db_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("store.sqlite"))
            .await
            .unwrap();
        // migrating again is a no-op
        migrate_to_latest(&db, ACTOR_DB_MIGRATIONS).await.unwrap();
        let tables: Vec<String> = db
            .run(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table' \
                     AND name NOT LIKE 'sqlite_%' ORDER BY name",
                )?;
                let names = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(names)
            })
            .await
            .unwrap();
        assert_eq!(
            tables,
            [
                "account_pref",
                "backlink",
                "blob",
                "migrations",
                "record",
                "record_blob",
                "repo_block",
                "repo_root"
            ]
        );
    }
}
