// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/actor-store/db

use crate::db::migrator::{migrate_to_latest, Migration, MigrationSet};
use crate::db::sqlite::{Db, Synchronous};
use anyhow::Result;
use std::path::Path;

pub type ActorDb = Db;

pub const ACTOR_DB_MIGRATIONS: &[Migration] = &[
    Migration {
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
    },
    Migration {
        name: "002",
        sql: "\
    CREATE TABLE space_repo (\
        space_uri TEXT PRIMARY KEY, \
        authority TEXT NOT NULL, \
        space_type TEXT NOT NULL, \
        skey TEXT NOT NULL, \
        rev TEXT NOT NULL, \
        lthash_state BLOB NOT NULL, \
        oplog_floor_rev TEXT, \
        deleted INTEGER NOT NULL DEFAULT 0, \
        created_at TEXT NOT NULL\
    );\
    CREATE TABLE space_record (\
        space_uri TEXT NOT NULL, \
        collection TEXT NOT NULL, \
        rkey TEXT NOT NULL, \
        cid TEXT NOT NULL, \
        rev TEXT NOT NULL, \
        value BLOB NOT NULL, \
        PRIMARY KEY (space_uri, collection, rkey)\
    );\
    CREATE TABLE space_oplog (\
        id INTEGER PRIMARY KEY AUTOINCREMENT, \
        space_uri TEXT NOT NULL, \
        rev TEXT NOT NULL, \
        collection TEXT NOT NULL, \
        rkey TEXT NOT NULL, \
        cid TEXT, \
        prev TEXT\
    );\
    CREATE INDEX space_oplog_space_idx ON space_oplog (space_uri, id);\
    CREATE TABLE space_blob_ref (\
        space_uri TEXT NOT NULL, \
        blob_cid TEXT NOT NULL, \
        collection TEXT NOT NULL, \
        rkey TEXT NOT NULL, \
        PRIMARY KEY (space_uri, blob_cid, collection, rkey)\
    );\
    CREATE TABLE space_repo_notify (\
        space_uri TEXT NOT NULL, \
        endpoint TEXT NOT NULL, \
        expires_at TEXT NOT NULL, \
        PRIMARY KEY (space_uri, endpoint)\
    );\
    CREATE TABLE space_def (\
        space_uri TEXT PRIMARY KEY, \
        space_type TEXT NOT NULL, \
        skey TEXT NOT NULL, \
        policy TEXT NOT NULL DEFAULT 'member-list', \
        app_access TEXT NOT NULL DEFAULT 'open', \
        allowed_clients TEXT, \
        managing_app TEXT, \
        deleted INTEGER NOT NULL DEFAULT 0, \
        created_at TEXT NOT NULL\
    );\
    CREATE TABLE space_member (\
        space_uri TEXT NOT NULL, \
        did TEXT NOT NULL, \
        PRIMARY KEY (space_uri, did)\
    );\
    CREATE TABLE space_writer (\
        space_uri TEXT NOT NULL, \
        did TEXT NOT NULL, \
        rev TEXT NOT NULL, \
        hash TEXT, \
        PRIMARY KEY (space_uri, did)\
    );\
    CREATE TABLE space_host_reg (\
        space_uri TEXT NOT NULL, \
        endpoint TEXT NOT NULL, \
        expires_at TEXT NOT NULL, \
        PRIMARY KEY (space_uri, endpoint)\
    );\
    CREATE TABLE space_used_jti (\
        jti TEXT PRIMARY KEY, \
        exp INTEGER NOT NULL\
    );",
    },
    // A notification is delivered with service auth addressed to the
    // subscriber, so a registration has to remember who the subscriber is.
    // Rows written before this carry no service identifier and keep the
    // pre-amendment behaviour (proposals#100).
    Migration {
        name: "003",
        sql: "\
    ALTER TABLE space_repo_notify ADD COLUMN service TEXT;\
    ALTER TABLE space_host_reg ADD COLUMN service TEXT;",
    },
    // Member rows carry when and at what rev they were added, which clients
    // surface; and each account keeps a local index of spaces it was enrolled
    // in, because listSpaces is how a member discovers a shared space at all --
    // a repo row only exists after the member's first write.
    Migration {
        name: "004",
        sql: "\
    ALTER TABLE space_member ADD COLUMN member_rev TEXT NOT NULL DEFAULT '';\
    ALTER TABLE space_member ADD COLUMN added_at TEXT NOT NULL DEFAULT '';\
    CREATE TABLE space_joined (\
        space_uri TEXT PRIMARY KEY, \
        authority TEXT NOT NULL, \
        space_type TEXT NOT NULL, \
        created_at TEXT NOT NULL\
    );",
    },
    // A write commits its publication intent with its blocks, so a crash
    // between the commit and the sequencer cannot lose or duplicate the
    // event; blob work is journaled the same way instead of queued in
    // memory; and repair steps are recognisable from the store itself.
    Migration {
        name: "005",
        sql: "\
    CREATE TABLE publish_intent (\
        id INTEGER PRIMARY KEY AUTOINCREMENT, \
        rev TEXT NOT NULL, \
        cid TEXT NOT NULL, \
        \"eventType\" TEXT NOT NULL, \
        event BLOB NOT NULL, \
        \"createdAt\" TEXT NOT NULL, \
        state TEXT NOT NULL DEFAULT 'pending', \
        \"seqFloor\" INTEGER, \
        seq INTEGER\
    );\
    CREATE INDEX publish_intent_state_idx ON publish_intent (state, id);\
    CREATE TABLE publish_ack (\
        \"intentId\" INTEGER PRIMARY KEY, \
        seq INTEGER NOT NULL, \
        \"ackedAt\" TEXT NOT NULL\
    );\
    CREATE TABLE blob_work (\
        id INTEGER PRIMARY KEY AUTOINCREMENT, \
        kind TEXT NOT NULL, \
        key TEXT NOT NULL, \
        cid TEXT, \
        state TEXT NOT NULL, \
        version INTEGER, \
        \"createdAt\" TEXT NOT NULL, \
        \"updatedAt\" TEXT NOT NULL\
    );\
    CREATE INDEX blob_work_state_idx ON blob_work (state, id);\
    CREATE TABLE blob_moderation (\
        cid TEXT PRIMARY KEY, \
        version INTEGER NOT NULL\
    );\
    CREATE TABLE repair_step (\
        \"repairId\" TEXT NOT NULL, \
        \"stepNo\" INTEGER NOT NULL, \
        rev TEXT NOT NULL, \
        PRIMARY KEY (\"repairId\", \"stepNo\")\
    );",
    },
];

/// The first migration is the reference actor-store schema; the rest are
/// rsky-only tables that the reference PDS never reads.
pub const ACTOR_DB_MIGRATION_SET: MigrationSet = MigrationSet {
    shared: ACTOR_DB_MIGRATIONS.split_at(1).0,
    local: ACTOR_DB_MIGRATIONS.split_at(1).1,
    legacy: None,
};

/// Opens a store for reading with the default durability.
pub fn get_db(location: impl AsRef<Path>) -> Result<ActorDb> {
    Db::open(location)
}

/// Opens a store for writing: every commit waits for stable storage, since
/// the write is acknowledged to the client and published from it.
pub async fn get_migrated_db(location: impl AsRef<Path>) -> Result<ActorDb> {
    let db = Db::open_with(location, Synchronous::Full)?;
    migrate_to_latest(&db, ACTOR_DB_MIGRATION_SET).await?;
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
        migrate_to_latest(&db, ACTOR_DB_MIGRATION_SET)
            .await
            .unwrap();
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
                "blob_moderation",
                "blob_work",
                "kysely_migration",
                "kysely_migration_lock",
                "migrations",
                "publish_ack",
                "publish_intent",
                "record",
                "record_blob",
                "repair_step",
                "repo_block",
                "repo_root",
                "space_blob_ref",
                "space_def",
                "space_host_reg",
                "space_joined",
                "space_member",
                "space_oplog",
                "space_record",
                "space_repo",
                "space_repo_notify",
                "space_used_jti",
                "space_writer"
            ]
        );
    }
}
