use crate::db::sqlite::Db;
use anyhow::Result;
use lexicon_cid::Cid;
use rusqlite::{params, OptionalExtension};

/// The root the account database records for `did`, as `(cid, rev)`.
pub async fn get_root(did: &str, db: &Db) -> Result<Option<(String, String)>> {
    let did = did.to_owned();
    db.run(move |conn| {
        Ok(conn
            .query_row(
                "SELECT cid, rev FROM repo_root WHERE did = ?1",
                [&did],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    })
    .await
}

pub async fn update_root(did: String, cid: Cid, rev: String, db: &Db) -> Result<()> {
    // @TODO balance risk of a race in the case of a long retry
    let now = rsky_common::now();
    let cid = cid.to_string();

    db.run(move |conn| {
        conn.execute(
            "INSERT INTO repo_root (did, cid, rev, \"indexedAt\") \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (did) DO UPDATE SET cid = excluded.cid, rev = excluded.rev",
            params![did, cid, rev, now],
        )?;
        Ok(())
    })
    .await
}
