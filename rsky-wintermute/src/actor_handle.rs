use crate::types::WintermuteError;
use deadpool_postgres::Client;

const HANDLE_LOCK_CLASS: i32 = 1;

/// Write an actor's handle, releasing it from any other actor first.
///
/// Writers of the same handle serialize on an advisory lock: two events for
/// one brand-new account otherwise insert the same row at the same instant,
/// and Postgres only retries the conflict on the arbiter index, so the
/// second insert fails on the handle index instead of becoming an update.
pub async fn upsert_actor_handle(
    client: &mut Client,
    did: &str,
    handle: Option<&str>,
    indexed_at: &str,
) -> Result<(), WintermuteError> {
    let txn = client.transaction().await?;
    if let Some(h) = handle {
        txn.execute(
            "SELECT pg_advisory_xact_lock($1, hashtext($2))",
            &[&HANDLE_LOCK_CLASS, &h],
        )
        .await?;
        let released = txn
            .execute(
                "UPDATE actor SET handle = NULL WHERE handle = $1 AND did != $2",
                &[&h, &did],
            )
            .await?;
        if released > 0 {
            tracing::info!("released handle {h} from a previous account for {did}");
        }
    }
    txn.execute(
        "INSERT INTO actor (did, handle, \"indexedAt\") VALUES ($1, $2, $3) \
         ON CONFLICT (did) DO UPDATE SET handle = EXCLUDED.handle, \
         \"indexedAt\" = EXCLUDED.\"indexedAt\"",
        &[&did, &handle, &indexed_at],
    )
    .await?;
    txn.commit().await?;
    Ok(())
}
