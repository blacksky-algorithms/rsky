use rusqlite::{Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::Path;

use crate::error::{HostError, Result};

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "001",
        "\
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
    ),
    (
        "002",
        "\
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
    ),
    // A notification is delivered with service auth addressed to the
    // subscriber, so a registration has to remember who the subscriber is.
    // Rows written before this carry no service identifier and keep the
    // pre-amendment behaviour (proposals#100).
    (
        "003",
        "\
    ALTER TABLE space_repo_notify ADD COLUMN service TEXT;\
    ALTER TABLE space_host_reg ADD COLUMN service TEXT;",
    ),
    // Member rows carry when and at what rev they were added, which clients
    // surface; and each account keeps a local index of spaces it was enrolled
    // in, because listSpaces is how a member discovers a shared space at all --
    // a repo row only exists after the member's first write.
    (
        "004",
        "\
    ALTER TABLE space_member ADD COLUMN member_rev TEXT NOT NULL DEFAULT '';\
    ALTER TABLE space_member ADD COLUMN added_at TEXT NOT NULL DEFAULT '';\
    CREATE TABLE space_joined (\
        space_uri TEXT PRIMARY KEY, \
        authority TEXT NOT NULL, \
        space_type TEXT NOT NULL, \
        created_at TEXT NOT NULL\
    );",
    ),
];

/// The first migration recreates the PDS's base schema. A store the PDS itself
/// created already has those tables under different bookkeeping, so applying it
/// there would both fail and be a write into a file this service does not own.
const BASELINE_MIGRATION: &str = "001";

/// Whether the base schema is already present, i.e. this file was created by
/// something other than these migrations.
fn baseline_present(tx: &rusqlite::Transaction<'_>) -> Result<bool> {
    tx.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'repo_root'",
        [],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(sql_err)
}

pub fn get_migrated_db(path: impl AsRef<Path>) -> Result<Connection> {
    let mut connection = Connection::open(path).map_err(sql_err)?;
    let transaction = connection.transaction().map_err(sql_err)?;
    transaction
        .execute_batch("CREATE TABLE IF NOT EXISTS migrations (name TEXT PRIMARY KEY, \"appliedAt\" TEXT NOT NULL)")
        .map_err(sql_err)?;
    let mut applied = transaction
        .prepare("SELECT name FROM migrations")
        .map_err(sql_err)?
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(sql_err)?
        .collect::<rusqlite::Result<HashSet<_>>>()
        .map_err(sql_err)?;
    if !applied.contains(BASELINE_MIGRATION) && baseline_present(&transaction)? {
        transaction
            .execute(
                "INSERT INTO migrations (name, \"appliedAt\") VALUES (?1, datetime('now'))",
                [BASELINE_MIGRATION],
            )
            .map_err(sql_err)?;
        applied.insert(BASELINE_MIGRATION.to_string());
    }
    for (name, sql) in MIGRATIONS {
        if !applied.contains(*name) {
            transaction.execute_batch(sql).map_err(sql_err)?;
            transaction
                .execute(
                    "INSERT INTO migrations (name, \"appliedAt\") VALUES (?1, datetime('now'))",
                    [name],
                )
                .map_err(sql_err)?;
        }
    }
    transaction.commit().map_err(sql_err)?;
    Ok(connection)
}

fn sql_err(error: rusqlite::Error) -> HostError {
    HostError::Store(error.to_string())
}
