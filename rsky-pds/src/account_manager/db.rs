// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/account-manager/db

use crate::db::migrator::{migrate_to_latest, Migration, MigrationSet};
use crate::db::sqlite::Db;
use anyhow::Result;
use rusqlite::Transaction;
use std::path::Path;

pub type AccountDb = Db;

/// The reference PDS account schema, migration for migration, so the same
/// `account.sqlite` can be served by either implementation. Column types are
/// the SQLite affinities Kysely emits for the reference migrations.
pub const ACCOUNT_DB_MIGRATIONS: &[Migration] = &[
    Migration {
        name: "001",
        sql: "\
    CREATE TABLE app_password (\
        did varchar NOT NULL, \
        name varchar NOT NULL, \
        \"passwordScrypt\" varchar NOT NULL, \
        \"createdAt\" varchar NOT NULL, \
        CONSTRAINT app_password_pkey PRIMARY KEY (did, name)\
    );\
    CREATE TABLE invite_code (\
        code varchar PRIMARY KEY, \
        \"availableUses\" integer NOT NULL, \
        disabled int2 DEFAULT 0, \
        \"forAccount\" varchar NOT NULL, \
        \"createdBy\" varchar NOT NULL, \
        \"createdAt\" varchar NOT NULL\
    );\
    CREATE INDEX invite_code_for_account_idx ON invite_code (\"forAccount\");\
    CREATE TABLE invite_code_use (\
        code varchar NOT NULL, \
        \"usedBy\" varchar NOT NULL, \
        \"usedAt\" varchar NOT NULL, \
        CONSTRAINT invite_code_use_pkey PRIMARY KEY (code, \"usedBy\")\
    );\
    CREATE TABLE refresh_token (\
        id varchar PRIMARY KEY, \
        did varchar NOT NULL, \
        \"expiresAt\" varchar NOT NULL, \
        \"nextId\" varchar, \
        \"appPasswordName\" varchar\
    );\
    CREATE INDEX refresh_token_did_idx ON refresh_token (did);\
    CREATE TABLE repo_root (\
        did varchar PRIMARY KEY, \
        cid varchar NOT NULL, \
        rev varchar NOT NULL, \
        \"indexedAt\" varchar NOT NULL\
    );\
    CREATE TABLE actor (\
        did varchar PRIMARY KEY, \
        handle varchar, \
        \"createdAt\" varchar NOT NULL, \
        \"takedownRef\" varchar\
    );\
    CREATE UNIQUE INDEX actor_handle_lower_idx ON actor (lower(\"handle\"));\
    CREATE INDEX actor_cursor_idx ON actor (\"createdAt\", did);\
    CREATE TABLE account (\
        did varchar PRIMARY KEY, \
        email varchar NOT NULL, \
        \"passwordScrypt\" varchar NOT NULL, \
        \"emailConfirmedAt\" varchar, \
        \"invitesDisabled\" int2 DEFAULT 0 NOT NULL\
    );\
    CREATE UNIQUE INDEX account_email_lower_idx ON account (lower(\"email\"));\
    CREATE TABLE email_token (\
        purpose varchar NOT NULL, \
        did varchar NOT NULL, \
        token varchar NOT NULL, \
        \"requestedAt\" varchar NOT NULL, \
        CONSTRAINT email_token_pkey PRIMARY KEY (purpose, did), \
        CONSTRAINT email_token_purpose_token_unique UNIQUE (purpose, token)\
    );",
    },
    Migration {
        name: "002",
        sql: "\
    ALTER TABLE actor ADD COLUMN \"deactivatedAt\" varchar;\
    ALTER TABLE actor ADD COLUMN \"deleteAfter\" varchar;",
    },
    Migration {
        name: "003",
        sql: "ALTER TABLE app_password ADD COLUMN privileged integer DEFAULT 0 NOT NULL;",
    },
    Migration {
        name: "004",
        sql: "\
    CREATE TABLE authorization_request (\
        id varchar PRIMARY KEY, \
        did varchar, \
        \"deviceId\" varchar, \
        \"clientId\" varchar NOT NULL, \
        \"clientAuth\" varchar NOT NULL, \
        parameters varchar NOT NULL, \
        \"expiresAt\" varchar NOT NULL, \
        code varchar\
    );\
    CREATE UNIQUE INDEX authorization_request_code_idx \
        ON authorization_request (code DESC) WHERE (code IS NOT NULL);\
    CREATE INDEX authorization_request_expires_at_idx \
        ON authorization_request (\"expiresAt\");\
    CREATE TABLE device (\
        id varchar PRIMARY KEY, \
        \"sessionId\" varchar NOT NULL, \
        \"userAgent\" varchar, \
        \"ipAddress\" varchar NOT NULL, \
        \"lastSeenAt\" varchar NOT NULL, \
        CONSTRAINT device_session_id_idx UNIQUE (\"sessionId\")\
    );\
    CREATE TABLE device_account (\
        did varchar NOT NULL, \
        \"deviceId\" varchar NOT NULL, \
        \"authenticatedAt\" varchar NOT NULL, \
        remember boolean NOT NULL, \
        \"authorizedClients\" varchar NOT NULL, \
        CONSTRAINT device_account_pk PRIMARY KEY (\"deviceId\", did), \
        CONSTRAINT device_account_device_id_fk FOREIGN KEY (\"deviceId\") \
            REFERENCES device (id) ON DELETE CASCADE ON UPDATE CASCADE\
    );\
    CREATE TABLE token (\
        id integer PRIMARY KEY AUTOINCREMENT, \
        did varchar NOT NULL, \
        \"tokenId\" varchar NOT NULL, \
        \"createdAt\" varchar NOT NULL, \
        \"updatedAt\" varchar NOT NULL, \
        \"expiresAt\" varchar NOT NULL, \
        \"clientId\" varchar NOT NULL, \
        \"clientAuth\" varchar NOT NULL, \
        \"deviceId\" varchar, \
        parameters varchar NOT NULL, \
        details varchar, \
        code varchar, \
        \"currentRefreshToken\" varchar, \
        CONSTRAINT token_current_refresh_token_unique_idx UNIQUE (\"currentRefreshToken\"), \
        CONSTRAINT token_id_unique_idx UNIQUE (\"tokenId\")\
    );\
    CREATE INDEX token_did_idx ON token (did);\
    CREATE UNIQUE INDEX token_code_idx ON token (code DESC) WHERE (code IS NOT NULL);\
    CREATE TABLE used_refresh_token (\
        \"refreshToken\" varchar PRIMARY KEY, \
        \"tokenId\" integer NOT NULL, \
        CONSTRAINT used_refresh_token_fk FOREIGN KEY (\"tokenId\") \
            REFERENCES token (id) ON DELETE CASCADE ON UPDATE CASCADE\
    );\
    CREATE INDEX used_refresh_token_id_idx ON used_refresh_token (\"tokenId\");",
    },
    Migration {
        name: "005",
        sql: "\
    CREATE TABLE IF NOT EXISTS account_device (\
        did varchar NOT NULL, \
        \"deviceId\" varchar NOT NULL, \
        \"createdAt\" varchar NOT NULL, \
        \"updatedAt\" varchar NOT NULL, \
        CONSTRAINT account_device_pk PRIMARY KEY (\"deviceId\", did), \
        CONSTRAINT account_device_did_fk FOREIGN KEY (did) \
            REFERENCES account (did) ON DELETE CASCADE ON UPDATE CASCADE, \
        CONSTRAINT account_device_device_id_fk FOREIGN KEY (\"deviceId\") \
            REFERENCES device (id) ON DELETE CASCADE ON UPDATE CASCADE\
    );\
    CREATE INDEX account_device_did_idx ON account_device (did);\
    CREATE TABLE authorized_client (\
        did varchar NOT NULL, \
        \"clientId\" varchar NOT NULL, \
        \"createdAt\" varchar NOT NULL, \
        \"updatedAt\" varchar NOT NULL, \
        data varchar NOT NULL, \
        CONSTRAINT authorized_client_pk PRIMARY KEY (did, \"clientId\"), \
        CONSTRAINT authorized_client_did_fk FOREIGN KEY (did) \
            REFERENCES account (did) ON DELETE CASCADE ON UPDATE CASCADE\
    );",
    },
    Migration {
        name: "006",
        sql: "\
    ALTER TABLE token ADD COLUMN scope varchar;\
    CREATE TABLE lexicon (\
        nsid varchar PRIMARY KEY, \
        \"createdAt\" varchar NOT NULL, \
        \"updatedAt\" varchar NOT NULL, \
        \"lastSucceededAt\" varchar, \
        uri varchar, \
        lexicon varchar\
    );",
    },
    Migration {
        name: "007",
        sql: "CREATE INDEX lexicon_failures_idx ON lexicon (\"updatedAt\" DESC) WHERE (\"lexicon\" IS NULL);",
    },
];

fn has_column(tx: &Transaction, table: &str, column: &str) -> Result<bool> {
    let mut stmt = tx.prepare(&format!("PRAGMA table_info({table})"))?;
    let found = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<String>, rusqlite::Error>>()?
        .iter()
        .any(|name| name == column);
    Ok(found)
}

/// Brings an account database created by rsky-pds 1.1.x to the reference
/// column set. Column affinities on those tables stay as rsky created them,
/// which only matters for databases the reference PDS never opens.
fn convert_legacy_account_db(tx: &Transaction) -> Result<()> {
    if has_column(tx, "account", "password")? {
        tx.execute_batch(
            "ALTER TABLE account RENAME COLUMN password TO \"passwordScrypt\";\
             ALTER TABLE account DROP COLUMN \"recoveryKey\";\
             ALTER TABLE account DROP COLUMN \"createdAt\";\
             ALTER TABLE account DROP COLUMN \"inviteNote\";",
        )?;
    }
    if has_column(tx, "app_password", "password")? {
        tx.execute_batch("ALTER TABLE app_password RENAME COLUMN password TO \"passwordScrypt\";")?;
    }
    if !crate::db::migrator::table_exists(tx, "device_account")? {
        tx.execute_batch(
            "CREATE TABLE device_account (\
                did varchar NOT NULL, \
                \"deviceId\" varchar NOT NULL, \
                \"authenticatedAt\" varchar NOT NULL, \
                remember boolean NOT NULL, \
                \"authorizedClients\" varchar NOT NULL, \
                CONSTRAINT device_account_pk PRIMARY KEY (\"deviceId\", did), \
                CONSTRAINT device_account_device_id_fk FOREIGN KEY (\"deviceId\") \
                    REFERENCES device (id) ON DELETE CASCADE ON UPDATE CASCADE\
            );",
        )?;
    }
    // the single legacy migration covered the whole reference set
    for name in ["002", "003", "004", "005", "006", "007"] {
        tx.execute(
            "INSERT OR IGNORE INTO migrations (name, \"appliedAt\") VALUES (?1, ?2)",
            rusqlite::params![name, rsky_common::now()],
        )?;
    }
    Ok(())
}

pub const ACCOUNT_DB_MIGRATION_SET: MigrationSet = MigrationSet {
    shared: ACCOUNT_DB_MIGRATIONS,
    local: &[],
    legacy: Some(convert_legacy_account_db),
};

pub fn get_db(location: impl AsRef<Path>) -> Result<AccountDb> {
    Db::open(location)
}

pub async fn get_migrated_db(location: impl AsRef<Path>) -> Result<AccountDb> {
    let db = get_db(location)?;
    migrate_to_latest(&db, ACCOUNT_DB_MIGRATION_SET).await?;
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    async fn table_names(db: &Db) -> Vec<String> {
        db.run(|conn| {
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
        .unwrap()
    }

    async fn columns(db: &Db, table: &'static str) -> Vec<String> {
        db.run(move |conn| {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
            let names = stmt
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<String>, rusqlite::Error>>()?;
            Ok(names)
        })
        .await
        .unwrap()
    }

    async fn ledger(db: &Db) -> Vec<String> {
        db.run(|conn| {
            let mut stmt = conn.prepare("SELECT name FROM kysely_migration ORDER BY name")?;
            let names = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<String>, rusqlite::Error>>()?;
            Ok(names)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn migrates_account_db_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("account.sqlite"))
            .await
            .unwrap();
        migrate_to_latest(&db, ACCOUNT_DB_MIGRATION_SET)
            .await
            .unwrap();
        assert_eq!(
            table_names(&db).await,
            [
                "account",
                "account_device",
                "actor",
                "app_password",
                "authorization_request",
                "authorized_client",
                "device",
                "device_account",
                "email_token",
                "invite_code",
                "invite_code_use",
                "kysely_migration",
                "kysely_migration_lock",
                "lexicon",
                "refresh_token",
                "repo_root",
                "token",
                "used_refresh_token"
            ]
        );
        assert_eq!(
            ledger(&db).await,
            ["001", "002", "003", "004", "005", "006", "007"]
        );
    }

    /// The column sets the reference PDS 0.5.27 leaves behind after its seven
    /// migrations, which the gatekeeper and both implementations depend on.
    #[tokio::test]
    async fn matches_reference_columns() {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("account.sqlite"))
            .await
            .unwrap();
        assert_eq!(
            columns(&db, "account").await,
            [
                "did",
                "email",
                "passwordScrypt",
                "emailConfirmedAt",
                "invitesDisabled"
            ]
        );
        assert_eq!(
            columns(&db, "actor").await,
            [
                "did",
                "handle",
                "createdAt",
                "takedownRef",
                "deactivatedAt",
                "deleteAfter"
            ]
        );
        assert_eq!(
            columns(&db, "app_password").await,
            ["did", "name", "passwordScrypt", "createdAt", "privileged"]
        );
        assert_eq!(
            columns(&db, "token").await,
            [
                "id",
                "did",
                "tokenId",
                "createdAt",
                "updatedAt",
                "expiresAt",
                "clientId",
                "clientAuth",
                "deviceId",
                "parameters",
                "details",
                "code",
                "currentRefreshToken",
                "scope"
            ]
        );
        assert_eq!(
            columns(&db, "lexicon").await,
            [
                "nsid",
                "createdAt",
                "updatedAt",
                "lastSucceededAt",
                "uri",
                "lexicon"
            ]
        );
    }

    /// A 1.1.x database whose tables already carry the reference column set
    /// only needs its ledger rows moved; the schema steps are skipped.
    #[tokio::test]
    async fn legacy_conversion_skips_already_converted_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = get_db(dir.path().join("account.sqlite")).unwrap();
        db.run(|conn| {
            conn.execute_batch(
                "CREATE TABLE migrations (name TEXT PRIMARY KEY, \"appliedAt\" TEXT NOT NULL);\
                 INSERT INTO migrations VALUES ('001', '2026-01-01T00:00:00.000Z');\
                 CREATE TABLE actor (did TEXT PRIMARY KEY, handle TEXT, \"createdAt\" TEXT NOT NULL, \
                    \"takedownRef\" TEXT, \"deactivatedAt\" TEXT, \"deleteAfter\" TEXT);\
                 CREATE TABLE account (did TEXT PRIMARY KEY, email TEXT NOT NULL, \
                    \"passwordScrypt\" TEXT NOT NULL, \"invitesDisabled\" INTEGER NOT NULL DEFAULT 0, \
                    \"emailConfirmedAt\" TEXT);\
                 CREATE TABLE app_password (did TEXT NOT NULL, name TEXT NOT NULL, \
                    \"passwordScrypt\" TEXT NOT NULL, \"createdAt\" TEXT NOT NULL, \
                    privileged INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (did, name));\
                 CREATE TABLE device (id TEXT PRIMARY KEY, \"sessionId\" TEXT NOT NULL, \"userAgent\" TEXT, \
                    \"ipAddress\" TEXT NOT NULL, \"lastSeenAt\" TEXT NOT NULL, UNIQUE (\"sessionId\"));\
                 CREATE TABLE device_account (did TEXT NOT NULL, \"deviceId\" TEXT NOT NULL, \
                    \"authenticatedAt\" TEXT NOT NULL, remember INTEGER NOT NULL, \
                    \"authorizedClients\" TEXT NOT NULL, PRIMARY KEY (\"deviceId\", did));\
                 INSERT INTO account (did, email, \"passwordScrypt\") VALUES ('did:plc:a', 'a@test', 'hash');",
            )?;
            Ok(())
        })
        .await
        .unwrap();
        migrate_to_latest(&db, ACCOUNT_DB_MIGRATION_SET)
            .await
            .unwrap();
        assert_eq!(
            ledger(&db).await,
            ["001", "002", "003", "004", "005", "006", "007"]
        );
        assert_eq!(
            columns(&db, "account").await,
            [
                "did",
                "email",
                "passwordScrypt",
                "invitesDisabled",
                "emailConfirmedAt"
            ]
        );
        let local_rows: i64 = db
            .run(
                |conn| Ok(conn.query_row("SELECT count(*) FROM migrations", [], |row| row.get(0))?),
            )
            .await
            .unwrap();
        assert_eq!(local_rows, 0);
    }

    /// An `account.sqlite` written by rsky-pds 1.1.x, before the ledgers were
    /// split, converts in place and reports nothing pending afterwards.
    #[tokio::test]
    async fn converts_rsky_1_1_account_db() {
        let dir = tempfile::tempdir().unwrap();
        let db = get_db(dir.path().join("account.sqlite")).unwrap();
        db.run(|conn| {
            conn.execute_batch(
                "CREATE TABLE migrations (name TEXT PRIMARY KEY, \"appliedAt\" TEXT NOT NULL);\
                 INSERT INTO migrations VALUES ('001', '2026-01-01T00:00:00.000Z');\
                 CREATE TABLE actor (did TEXT PRIMARY KEY, handle TEXT, \"createdAt\" TEXT NOT NULL, \
                    \"takedownRef\" TEXT, \"deactivatedAt\" TEXT, \"deleteAfter\" TEXT);\
                 CREATE TABLE account (did TEXT PRIMARY KEY, email TEXT NOT NULL, \"recoveryKey\" TEXT, \
                    password TEXT NOT NULL, \"createdAt\" TEXT NOT NULL, \
                    \"invitesDisabled\" INTEGER NOT NULL DEFAULT 0, \"emailConfirmedAt\" TEXT, \"inviteNote\" TEXT);\
                 CREATE TABLE app_password (did TEXT NOT NULL, name TEXT NOT NULL, password TEXT NOT NULL, \
                    \"createdAt\" TEXT NOT NULL, privileged INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (did, name));\
                 CREATE TABLE device (id TEXT PRIMARY KEY, \"sessionId\" TEXT NOT NULL, \"userAgent\" TEXT, \
                    \"ipAddress\" TEXT NOT NULL, \"lastSeenAt\" TEXT NOT NULL, UNIQUE (\"sessionId\"));\
                 INSERT INTO actor VALUES ('did:plc:a', 'a.test', '2026-01-01T00:00:00.000Z', NULL, NULL, NULL);\
                 INSERT INTO account (did, email, password, \"createdAt\") \
                    VALUES ('did:plc:a', 'a@test', 'hash', '2026-01-01T00:00:00.000Z');\
                 INSERT INTO app_password (did, name, password, \"createdAt\") \
                    VALUES ('did:plc:a', 'phone', 'apphash', '2026-01-01T00:00:00.000Z');",
            )?;
            Ok(())
        })
        .await
        .unwrap();
        migrate_to_latest(&db, ACCOUNT_DB_MIGRATION_SET)
            .await
            .unwrap();
        assert_eq!(
            ledger(&db).await,
            ["001", "002", "003", "004", "005", "006", "007"]
        );
        assert_eq!(
            columns(&db, "account").await,
            [
                "did",
                "email",
                "passwordScrypt",
                "invitesDisabled",
                "emailConfirmedAt"
            ]
        );
        assert!(columns(&db, "app_password")
            .await
            .contains(&"passwordScrypt".to_string()));
        assert!(table_names(&db)
            .await
            .contains(&"device_account".to_string()));
        let (hash, app_hash): (String, String) = db
            .run(|conn| {
                let hash = conn.query_row(
                    "SELECT \"passwordScrypt\" FROM account WHERE did = 'did:plc:a'",
                    [],
                    |row| row.get(0),
                )?;
                let app_hash = conn.query_row(
                    "SELECT \"passwordScrypt\" FROM app_password WHERE did = 'did:plc:a'",
                    [],
                    |row| row.get(0),
                )?;
                Ok((hash, app_hash))
            })
            .await
            .unwrap();
        assert_eq!((hash.as_str(), app_hash.as_str()), ("hash", "apphash"));
        let local: i64 = db
            .run(
                |conn| Ok(conn.query_row("SELECT count(*) FROM migrations", [], |row| row.get(0))?),
            )
            .await
            .unwrap();
        assert_eq!(local, 0);
        migrate_to_latest(&db, ACCOUNT_DB_MIGRATION_SET)
            .await
            .unwrap();
        let _ = params![];
    }
}
