// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/account-manager/db

use crate::db::migrator::{migrate_to_latest, Migration};
use crate::db::sqlite::Db;
use anyhow::Result;
use std::path::Path;

pub type AccountDb = Db;

pub const ACCOUNT_DB_MIGRATIONS: &[Migration] = &[Migration {
    name: "001",
    sql: "\
    CREATE TABLE actor (\
        did TEXT PRIMARY KEY, \
        handle TEXT, \
        \"createdAt\" TEXT NOT NULL, \
        \"takedownRef\" TEXT, \
        \"deactivatedAt\" TEXT, \
        \"deleteAfter\" TEXT\
    );\
    CREATE UNIQUE INDEX actor_handle_lower_idx ON actor (lower(handle));\
    CREATE INDEX actor_cursor_idx ON actor (\"createdAt\", did);\
    CREATE TABLE account (\
        did TEXT PRIMARY KEY, \
        email TEXT NOT NULL, \
        \"recoveryKey\" TEXT, \
        password TEXT NOT NULL, \
        \"createdAt\" TEXT NOT NULL, \
        \"invitesDisabled\" INTEGER NOT NULL DEFAULT 0, \
        \"emailConfirmedAt\" TEXT, \
        \"inviteNote\" TEXT\
    );\
    CREATE UNIQUE INDEX account_email_lower_idx ON account (lower(email));\
    CREATE TABLE app_password (\
        did TEXT NOT NULL, \
        name TEXT NOT NULL, \
        password TEXT NOT NULL, \
        \"createdAt\" TEXT NOT NULL, \
        privileged INTEGER NOT NULL DEFAULT 0, \
        PRIMARY KEY (did, name)\
    );\
    CREATE TABLE refresh_token (\
        id TEXT PRIMARY KEY, \
        did TEXT NOT NULL, \
        \"expiresAt\" TEXT NOT NULL, \
        \"nextId\" TEXT, \
        \"appPasswordName\" TEXT\
    );\
    CREATE INDEX refresh_token_did_idx ON refresh_token (did);\
    CREATE TABLE repo_root (\
        did TEXT PRIMARY KEY, \
        cid TEXT NOT NULL, \
        rev TEXT NOT NULL, \
        \"indexedAt\" TEXT NOT NULL\
    );\
    CREATE TABLE invite_code (\
        code TEXT PRIMARY KEY, \
        \"availableUses\" INTEGER NOT NULL, \
        disabled INTEGER NOT NULL DEFAULT 0, \
        \"forAccount\" TEXT NOT NULL, \
        \"createdBy\" TEXT NOT NULL, \
        \"createdAt\" TEXT NOT NULL\
    );\
    CREATE INDEX invite_code_for_account_idx ON invite_code (\"forAccount\");\
    CREATE TABLE invite_code_use (\
        code TEXT NOT NULL, \
        \"usedBy\" TEXT NOT NULL, \
        \"usedAt\" TEXT NOT NULL, \
        PRIMARY KEY (code, \"usedBy\")\
    );\
    CREATE TABLE email_token (\
        purpose TEXT NOT NULL, \
        did TEXT NOT NULL, \
        token TEXT NOT NULL, \
        \"requestedAt\" TEXT NOT NULL, \
        PRIMARY KEY (purpose, did), \
        UNIQUE (purpose, token)\
    );\
    CREATE TABLE authorization_request (\
        id TEXT PRIMARY KEY, \
        did TEXT, \
        \"deviceId\" TEXT, \
        \"clientId\" TEXT NOT NULL, \
        \"clientAuth\" TEXT NOT NULL, \
        parameters TEXT NOT NULL, \
        \"expiresAt\" TEXT NOT NULL, \
        code TEXT\
    );\
    CREATE UNIQUE INDEX authorization_request_code_idx \
        ON authorization_request (code DESC) WHERE code IS NOT NULL;\
    CREATE INDEX authorization_request_expires_at_idx \
        ON authorization_request (\"expiresAt\");\
    CREATE TABLE device (\
        id TEXT PRIMARY KEY, \
        \"sessionId\" TEXT NOT NULL, \
        \"userAgent\" TEXT, \
        \"ipAddress\" TEXT NOT NULL, \
        \"lastSeenAt\" TEXT NOT NULL, \
        UNIQUE (\"sessionId\")\
    );\
    CREATE TABLE account_device (\
        did TEXT NOT NULL, \
        \"deviceId\" TEXT NOT NULL, \
        \"createdAt\" TEXT NOT NULL, \
        \"updatedAt\" TEXT NOT NULL, \
        PRIMARY KEY (\"deviceId\", did), \
        FOREIGN KEY (did) REFERENCES account (did) \
            ON DELETE CASCADE ON UPDATE CASCADE, \
        FOREIGN KEY (\"deviceId\") REFERENCES device (id) \
            ON DELETE CASCADE ON UPDATE CASCADE\
    );\
    CREATE INDEX account_device_did_idx ON account_device (did);\
    CREATE TABLE authorized_client (\
        did TEXT NOT NULL, \
        \"clientId\" TEXT NOT NULL, \
        \"createdAt\" TEXT NOT NULL, \
        \"updatedAt\" TEXT NOT NULL, \
        data TEXT NOT NULL, \
        PRIMARY KEY (did, \"clientId\"), \
        FOREIGN KEY (did) REFERENCES account (did) \
            ON DELETE CASCADE ON UPDATE CASCADE\
    );\
    CREATE TABLE token (\
        id INTEGER PRIMARY KEY AUTOINCREMENT, \
        did TEXT NOT NULL, \
        \"tokenId\" TEXT NOT NULL, \
        \"createdAt\" TEXT NOT NULL, \
        \"updatedAt\" TEXT NOT NULL, \
        \"expiresAt\" TEXT NOT NULL, \
        \"clientId\" TEXT NOT NULL, \
        \"clientAuth\" TEXT NOT NULL, \
        \"deviceId\" TEXT, \
        parameters TEXT NOT NULL, \
        details TEXT, \
        code TEXT, \
        \"currentRefreshToken\" TEXT, \
        scope TEXT, \
        UNIQUE (\"currentRefreshToken\"), \
        UNIQUE (\"tokenId\")\
    );\
    CREATE INDEX token_did_idx ON token (did);\
    CREATE UNIQUE INDEX token_code_idx ON token (code DESC) WHERE code IS NOT NULL;\
    CREATE TABLE used_refresh_token (\
        \"refreshToken\" TEXT PRIMARY KEY, \
        \"tokenId\" INTEGER NOT NULL, \
        FOREIGN KEY (\"tokenId\") REFERENCES token (id) \
            ON DELETE CASCADE ON UPDATE CASCADE\
    );\
    CREATE INDEX used_refresh_token_id_idx ON used_refresh_token (\"tokenId\");\
    CREATE TABLE lexicon (\
        nsid TEXT PRIMARY KEY, \
        \"createdAt\" TEXT NOT NULL, \
        \"updatedAt\" TEXT NOT NULL, \
        \"lastSucceededAt\" TEXT, \
        uri TEXT, \
        lexicon TEXT\
    );\
    CREATE INDEX lexicon_failures_idx ON lexicon (\"updatedAt\" DESC) WHERE lexicon IS NULL;",
}];

pub fn get_db(location: impl AsRef<Path>) -> Result<AccountDb> {
    Db::open(location)
}

pub async fn get_migrated_db(location: impl AsRef<Path>) -> Result<AccountDb> {
    let db = get_db(location)?;
    migrate_to_latest(&db, ACCOUNT_DB_MIGRATIONS).await?;
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrates_account_db_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("account.sqlite"))
            .await
            .unwrap();
        // migrating again is a no-op
        migrate_to_latest(&db, ACCOUNT_DB_MIGRATIONS).await.unwrap();
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
                "account",
                "account_device",
                "actor",
                "app_password",
                "authorization_request",
                "authorized_client",
                "device",
                "email_token",
                "invite_code",
                "invite_code_use",
                "lexicon",
                "migrations",
                "refresh_token",
                "repo_root",
                "token",
                "used_refresh_token"
            ]
        );
    }
}
