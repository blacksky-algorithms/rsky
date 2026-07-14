// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/sequencer/db

use crate::db::migrator::{migrate_to_latest, Migration};
use crate::db::sqlite::Db;
use anyhow::Result;
use std::path::Path;

pub type SequencerDb = Db;

pub const SEQUENCER_DB_MIGRATIONS: &[Migration] = &[Migration {
    name: "001",
    sql: "\
    CREATE TABLE repo_seq (\
        seq INTEGER PRIMARY KEY AUTOINCREMENT, \
        did TEXT NOT NULL, \
        \"eventType\" TEXT NOT NULL, \
        event BLOB NOT NULL, \
        invalidated INTEGER NOT NULL DEFAULT 0, \
        \"sequencedAt\" TEXT NOT NULL\
    );\
    CREATE INDEX repo_seq_did_idx ON repo_seq (did);\
    CREATE INDEX repo_seq_event_type_idx ON repo_seq (\"eventType\");\
    CREATE INDEX repo_seq_sequenced_at_index ON repo_seq (\"sequencedAt\");",
}];

pub fn get_db(location: impl AsRef<Path>) -> Result<SequencerDb> {
    Db::open(location)
}

pub async fn get_migrated_db(location: impl AsRef<Path>) -> Result<SequencerDb> {
    let db = get_db(location)?;
    migrate_to_latest(&db, SEQUENCER_DB_MIGRATIONS).await?;
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrates_sequencer_db_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("sequencer.sqlite"))
            .await
            .unwrap();
        // migrating again is a no-op
        migrate_to_latest(&db, SEQUENCER_DB_MIGRATIONS)
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
        assert_eq!(tables, ["migrations", "repo_seq"]);
    }
}
