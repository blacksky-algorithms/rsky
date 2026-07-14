use crate::db::sqlite::Db;
use anyhow::Result;
use lexicon_cid::Cid;
use rusqlite::params;

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
