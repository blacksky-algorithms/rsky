use crate::db::DbConn;
use anyhow::Result;
use diesel::*;
use lexicon_cid::Cid;
use rsky_common;

pub async fn update_root(did: String, cid: Cid, rev: String, db: &DbConn) -> Result<()> {
    // @TODO balance risk of a race in the case of a long retry
    use crate::schema::pds::repo_root::dsl as RepoRootSchema;

    let now = rsky_common::now();

    db.run(move |conn| {
        insert_into(RepoRootSchema::repo_root)
            .values((
                RepoRootSchema::did.eq(did),
                RepoRootSchema::cid.eq(cid.to_string()),
                RepoRootSchema::rev.eq(rev.clone()),
                RepoRootSchema::indexedAt.eq(now),
            ))
            .on_conflict(RepoRootSchema::did)
            .do_update()
            .set((
                RepoRootSchema::cid.eq(cid.to_string()),
                RepoRootSchema::rev.eq(rev),
            ))
            .execute(conn)
    })
    .await?;

    Ok(())
}
