use crate::graph::FollowGraph;
use crate::types::GraphError;
use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
use tokio_postgres::NoTls;

pub async fn bulk_load_follows(database_url: &str, graph: &FollowGraph) -> Result<(), GraphError> {
    let mut pg_config = Config::new();
    pg_config.url = Some(database_url.to_owned());
    pg_config.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    pg_config.pool = Some(deadpool_postgres::PoolConfig::new(2));

    let pool = pg_config
        .create_pool(Some(Runtime::Tokio1), NoTls)
        .map_err(|e| GraphError::Other(format!("pool creation failed: {e}")))?;

    let client = pool
        .get()
        .await
        .map_err(|e| GraphError::Other(format!("pool get failed: {e}")))?;

    // Set search path and statement timeout
    client
        .execute("SET search_path TO bsky", &[])
        .await
        .map_err(|e| GraphError::Other(format!("set search_path failed: {e}")))?;
    client
        .execute("SET statement_timeout = '0'", &[])
        .await
        .map_err(|e| GraphError::Other(format!("set timeout failed: {e}")))?;

    tracing::info!("starting bulk load from PostgreSQL follow table");

    // Begin transaction for cursor
    client
        .execute("BEGIN", &[])
        .await
        .map_err(|e| GraphError::Other(format!("begin failed: {e}")))?;

    // Stream follows using a cursor to avoid loading everything into memory
    client
        .execute(
            "DECLARE follow_cursor CURSOR FOR SELECT creator, \"subjectDid\" FROM follow",
            &[],
        )
        .await
        .map_err(|e| GraphError::Other(format!("declare cursor failed: {e}")))?;

    let mut total: u64 = 0;
    let batch_size = 100_000;
    let start = std::time::Instant::now();

    loop {
        let rows = client
            .query(&format!("FETCH {batch_size} FROM follow_cursor"), &[])
            .await
            .map_err(|e| GraphError::Other(format!("fetch failed: {e}")))?;

        if rows.is_empty() {
            break;
        }

        for row in &rows {
            let creator: String = row.get(0);
            let subject: String = row.get(1);
            graph.add_follow(&creator, &subject);
        }

        total += rows.len() as u64;

        if total % 1_000_000 == 0 {
            let elapsed = start.elapsed().as_secs();
            let rate = if elapsed > 0 { total / elapsed } else { 0 };
            tracing::info!(
                "bulk load: {} follows loaded ({} follows/sec), {} users",
                total,
                rate,
                graph.user_count()
            );

            crate::metrics::GRAPH_USERS_TOTAL.set(graph.user_count() as i64);
            crate::metrics::GRAPH_FOLLOWS_TOTAL.set(total as i64);
        }
    }

    client.execute("CLOSE follow_cursor", &[]).await.ok();
    client.execute("COMMIT", &[]).await.ok();

    let elapsed = start.elapsed();
    tracing::info!(
        "bulk load complete: {} follows in {:.1}s ({} follows/sec)",
        total,
        elapsed.as_secs_f64(),
        total / elapsed.as_secs().max(1)
    );

    crate::metrics::GRAPH_USERS_TOTAL.set(graph.user_count() as i64);
    crate::metrics::GRAPH_FOLLOWS_TOTAL.set(total as i64);

    Ok(())
}
