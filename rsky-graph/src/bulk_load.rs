use crate::graph::FollowGraph;
use crate::types::GraphError;
use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_postgres::NoTls;

const PROGRESS_EVERY: u64 = 1_000_000;

fn report_progress(total: u64, start: std::time::Instant, graph: &FollowGraph) {
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

fn finalize(total: u64, start: std::time::Instant, graph: &FollowGraph) {
    let elapsed = start.elapsed();
    tracing::info!(
        "bulk load complete: {} follows in {:.1}s ({} follows/sec)",
        total,
        elapsed.as_secs_f64(),
        total / elapsed.as_secs().max(1)
    );
    crate::metrics::GRAPH_USERS_TOTAL.set(graph.user_count() as i64);
    crate::metrics::GRAPH_FOLLOWS_TOTAL.set(total as i64);
}

/// Load follows from a CSV file produced by `\copy` from PostgreSQL.
///
/// This is the preferred load path. It performs zero PostgreSQL work at load time —
/// the CSV is built once via `\copy` (a short-lived read snapshot, no row locks) and
/// can then be consumed at full local-disk speed without any further DB pressure.
///
/// Supports two CSV shapes (no header, comma-separated):
/// - **2 columns**: `creator,subjectDid`
/// - **3 columns**: `creator,rkey,subjectDid` — recommended, lets firehose
///   delete events resolve the subject to remove.
pub async fn bulk_load_from_file(path: &Path, graph: &FollowGraph) -> Result<(), GraphError> {
    tracing::info!("bulk-loading follows from file: {}", path.display());

    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| GraphError::Other(format!("open {} failed: {e}", path.display())))?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut total: u64 = 0;
    let mut with_rkey: u64 = 0;
    let start = std::time::Instant::now();

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| GraphError::Other(format!("csv read failed: {e}")))?
    {
        if line.is_empty() {
            continue;
        }
        match parse_csv_line(&line) {
            Some(CsvFollow::TwoCol { creator, subject }) => {
                graph.add_follow(creator, subject);
            }
            Some(CsvFollow::ThreeCol {
                creator,
                rkey,
                subject,
            }) => {
                graph.add_follow_with_rkey(creator, rkey, subject);
                with_rkey += 1;
            }
            None => {
                tracing::warn!("skipping malformed CSV line: {line}");
                continue;
            }
        }
        total += 1;
        if total % PROGRESS_EVERY == 0 {
            report_progress(total, start, graph);
        }
    }

    if with_rkey > 0 {
        tracing::info!("indexed {with_rkey} follows by rkey for delete resolution");
    } else {
        tracing::warn!(
            "CSV had no rkey column -- pre-snapshot follows will not be deletable via firehose \
             until the next snapshot rebuild"
        );
    }

    finalize(total, start, graph);
    Ok(())
}

enum CsvFollow<'a> {
    TwoCol {
        creator: &'a str,
        subject: &'a str,
    },
    ThreeCol {
        creator: &'a str,
        rkey: &'a str,
        subject: &'a str,
    },
}

fn parse_csv_line(line: &str) -> Option<CsvFollow<'_>> {
    // DIDs and rkeys we care about never contain commas or quotes, so a fast
    // split is correct. Strip surrounding quotes defensively.
    let mut parts = line.split(',');
    let a = strip_csv_quotes(parts.next()?);
    let b = strip_csv_quotes(parts.next()?);
    match parts.next() {
        Some(c) => {
            let c = strip_csv_quotes(c);
            if parts.next().is_some() {
                return None;
            }
            Some(CsvFollow::ThreeCol {
                creator: a,
                rkey: b,
                subject: c,
            })
        }
        None => Some(CsvFollow::TwoCol {
            creator: a,
            subject: b,
        }),
    }
}

fn strip_csv_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Load follows from PostgreSQL using **keyset pagination with short transactions**.
///
/// This replaces the prior `DECLARE CURSOR ... FETCH` approach which held a single
/// long-running transaction across the entire 3.4 B-row table — that snapshot
/// blocked `VACUUM` and stressed IO unboundedly, hurting the live appview.
///
/// Each iteration runs an autonomous short query bounded by `LIMIT batch_size`,
/// keyset-paged on `(creator, "subjectDid")`. Between batches we sleep
/// `throttle_ms` to cap IO. Either may be tuned via env vars.
pub async fn bulk_load_keyset(database_url: &str, graph: &FollowGraph) -> Result<(), GraphError> {
    let batch_size: i64 = std::env::var("GRAPH_LOAD_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50_000);
    let throttle_ms: u64 = std::env::var("GRAPH_LOAD_THROTTLE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);

    tracing::info!(
        "starting keyset bulk load (batch_size={}, throttle_ms={})",
        batch_size,
        throttle_ms
    );

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

    client
        .execute("SET search_path TO bsky", &[])
        .await
        .map_err(|e| GraphError::Other(format!("set search_path failed: {e}")))?;
    // Generous per-statement cap, but each query reads at most batch_size rows
    // and finishes well within this. Prevents a single bad batch from running away.
    client
        .execute("SET statement_timeout = '60s'", &[])
        .await
        .map_err(|e| GraphError::Other(format!("set timeout failed: {e}")))?;

    let mut total: u64 = 0;
    let start = std::time::Instant::now();
    let mut last_creator = String::new();
    let mut last_subject = String::new();

    let initial_stmt = client
        .prepare(
            "SELECT creator, \"subjectDid\" FROM follow \
             ORDER BY creator, \"subjectDid\" LIMIT $1::bigint",
        )
        .await
        .map_err(|e| GraphError::Other(format!("prepare initial failed: {e}")))?;

    let page_stmt = client
        .prepare(
            "SELECT creator, \"subjectDid\" FROM follow \
             WHERE (creator, \"subjectDid\") > ($1, $2) \
             ORDER BY creator, \"subjectDid\" LIMIT $3::bigint",
        )
        .await
        .map_err(|e| GraphError::Other(format!("prepare page failed: {e}")))?;

    loop {
        let rows = if total == 0 {
            client
                .query(&initial_stmt, &[&batch_size])
                .await
                .map_err(|e| GraphError::Other(format!("initial query failed: {e}")))?
        } else {
            client
                .query(&page_stmt, &[&last_creator, &last_subject, &batch_size])
                .await
                .map_err(|e| GraphError::Other(format!("page query failed: {e}")))?
        };

        if rows.is_empty() {
            break;
        }

        for row in &rows {
            let creator: String = row.get(0);
            let subject: String = row.get(1);
            graph.add_follow(&creator, &subject);
            last_creator = creator;
            last_subject = subject;
        }

        total += rows.len() as u64;

        if total % PROGRESS_EVERY == 0 {
            report_progress(total, start, graph);
        }

        if throttle_ms > 0 {
            tokio::time::sleep(Duration::from_millis(throttle_ms)).await;
        }
    }

    finalize(total, start, graph);
    Ok(())
}
