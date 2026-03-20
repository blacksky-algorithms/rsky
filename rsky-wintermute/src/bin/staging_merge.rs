//! Merge sorted staging data into production PostgreSQL.
//!
//! Reads from UNLOGGED staging tables (on a temp server), sorts by primary key,
//! and inserts into production using COPY + INSERT ON CONFLICT DO NOTHING.
//! The sorted key order gives sequential B-tree access on production indexes.

use std::time::Instant;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use tokio_postgres::NoTls;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Debug, Parser)]
#[command(name = "staging_merge")]
#[command(about = "Merge staging tables into production with sorted INSERT ON CONFLICT DO NOTHING")]
struct Args {
    /// Staging database URL
    #[arg(long, env = "STAGING_DATABASE_URL")]
    staging_url: String,

    /// Production database URL
    #[arg(long, env = "DATABASE_URL")]
    production_url: String,

    /// Rows per merge batch
    #[arg(long, default_value = "500000")]
    batch_size: i64,

    /// Production pool size
    #[arg(long, default_value = "4")]
    pool_size: usize,

    /// Skip these tables (comma-separated, e.g. --skip record)
    #[arg(long, default_value = "")]
    skip: String,

    #[command(subcommand)]
    command: MergeCommand,
}

#[derive(Debug, Subcommand)]
enum MergeCommand {
    /// Merge all tables (use --skip to exclude)
    All,
    /// Show staging table row counts
    Status,
    /// Merge a specific table
    Table {
        /// Table name (record, post, like, follow, repost, feed_item, block, profile, embed_image, embed_video)
        name: String,
    },
    /// Merge specific tables in order (comma-separated)
    Tables {
        /// Comma-separated table names
        names: String,
    },
}

const TABLES: &[&str] = &[
    "record",
    "post",
    "like",
    "follow",
    "repost",
    "feed_item",
    "block",
    "profile",
    "post_embed_image",
    "post_embed_video",
];

fn create_pool(url: &str, size: usize) -> Result<Pool> {
    let mut cfg = Config::new();
    cfg.url = Some(url.to_owned());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    cfg.pool = Some(deadpool_postgres::PoolConfig::new(size));
    Ok(cfg.create_pool(Some(Runtime::Tokio1), NoTls)?)
}

fn main() -> Result<()> {
    color_eyre::install()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async { run(args).await })
}

async fn run(args: Args) -> Result<()> {
    let staging_pool = create_pool(&args.staging_url, 2)?;
    let production_pool = create_pool(&args.production_url, args.pool_size)?;

    match args.command {
        MergeCommand::Status => {
            let client = staging_pool.get().await?;
            for table in TABLES {
                let staging_name = format!("staging_{table}");
                let row = client
                    .query_one(&format!("SELECT COUNT(*) FROM {staging_name}"), &[])
                    .await?;
                let count: i64 = row.get(0);
                tracing::info!("{staging_name}: {count} rows");
            }
        }
        MergeCommand::All => {
            let skip: Vec<&str> = args
                .skip
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            for table in TABLES {
                if skip.contains(table) {
                    tracing::info!("skipping {table} (--skip)");
                    continue;
                }
                merge_table(&staging_pool, &production_pool, table, args.batch_size).await?;
            }
        }
        MergeCommand::Table { name } => {
            merge_table(&staging_pool, &production_pool, &name, args.batch_size).await?;
        }
        MergeCommand::Tables { names } => {
            for name in names.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                merge_table(&staging_pool, &production_pool, name, args.batch_size).await?;
            }
        }
    }

    Ok(())
}

async fn merge_table(
    staging_pool: &Pool,
    production_pool: &Pool,
    table: &str,
    batch_size: i64,
) -> Result<()> {
    let staging_name = format!("staging_{table}");

    let staging_client = staging_pool.get().await?;
    let production_client = production_pool.get().await?;

    // Override production statement timeout for merge batches
    production_client
        .execute("SET statement_timeout = '600s'", &[])
        .await?;

    tracing::info!("merging {table} from {staging_name} (streaming, no count/index)");

    // Stream rows via cursor without sorting on the staging side.
    // The INSERT INTO production has ORDER BY uri which sorts each batch
    // on the production temp table (small, in-memory sort).
    staging_client.execute("BEGIN", &[]).await?;
    staging_client
        .execute(
            &format!("DECLARE merge_cursor CURSOR FOR SELECT * FROM {staging_name}"),
            &[],
        )
        .await?;

    let mut merged: i64 = 0;
    loop {
        let batch_start = Instant::now();
        let rows = staging_client
            .query(&format!("FETCH {batch_size} FROM merge_cursor"), &[])
            .await?;

        if rows.is_empty() {
            break;
        }

        let batch_count = rows.len() as i64;

        // Build the merge INSERT using temp table + COPY + INSERT ON CONFLICT DO NOTHING
        // with ORDER BY to preserve sort order through the merge path.
        merge_batch(&production_client, table, &rows).await?;

        merged += batch_count;
        let elapsed = batch_start.elapsed().as_millis();
        let rate = if elapsed > 0 {
            batch_count * 1000 / elapsed as i64
        } else {
            0
        };
        tracing::info!(
            "merged {merged} rows for {table} ({batch_count} in {elapsed}ms, {rate} rows/sec)"
        );
    }

    staging_client.execute("CLOSE merge_cursor", &[]).await?;
    staging_client.execute("COMMIT", &[]).await?;

    tracing::info!("merge complete for {table}: {merged} rows");
    Ok(())
}

async fn merge_batch(
    client: &deadpool_postgres::Client,
    table: &str,
    rows: &[tokio_postgres::Row],
) -> Result<()> {
    use futures::SinkExt;
    use futures::pin_mut;
    use std::io::Write;

    // Create temp table matching the staging schema
    let (temp_ddl, copy_cols, insert_sql) = merge_sql(table);

    client.execute(&temp_ddl, &[]).await?;
    client
        .execute(&format!("TRUNCATE _merge_{table}"), &[])
        .await?;

    // COPY rows into temp table
    let copy_stmt = client
        .copy_in(&format!(
            "COPY _merge_{table} ({copy_cols}) FROM STDIN WITH (FORMAT text, DELIMITER E'\\t', NULL '\\N')"
        ))
        .await?;
    let sink = copy_stmt;
    pin_mut!(sink);

    let mut buffer = Vec::with_capacity(rows.len() * 200);
    let num_cols = rows.first().map_or(0, |r| r.len());
    for row in rows {
        for col in 0..num_cols {
            if col > 0 {
                write!(buffer, "\t").ok();
            }
            let val: Option<String> = row.try_get(col).ok().flatten();
            match val {
                Some(v) => {
                    let escaped = v
                        .replace('\\', "\\\\")
                        .replace('\t', "\\t")
                        .replace('\n', "\\n")
                        .replace('\r', "\\r");
                    write!(buffer, "{escaped}").ok();
                }
                None => {
                    write!(buffer, "\\N").ok();
                }
            }
        }
        writeln!(buffer).ok();
    }

    sink.send(bytes::Bytes::from(buffer)).await?;
    sink.close().await?;

    // Insert into production with sort-preserving ORDER BY
    client.execute(&insert_sql, &[]).await?;

    Ok(())
}

/// Returns (temp table DDL, COPY column list, INSERT SQL with ORDER BY and ON CONFLICT)
fn merge_sql(table: &str) -> (String, String, String) {
    match table {
        "actor" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_actor (did text)".into(),
            "did".into(),
            "INSERT INTO actor (did, \"indexedAt\")
             SELECT did, '1970-01-01T00:00:00Z' FROM _merge_actor
             ORDER BY did
             ON CONFLICT (did) DO NOTHING".into(),
        ),
        "record" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_record (
                uri text, cid text, did text, json text, rev text, indexed_at text
            )".into(),
            "uri, cid, did, json, rev, indexed_at".into(),
            "INSERT INTO record (uri, cid, did, json, rev, \"indexedAt\")
             SELECT uri, cid, did, json, rev, indexed_at FROM _merge_record
             ORDER BY uri
             ON CONFLICT (uri) DO NOTHING".into(),
        ),
        "post" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_post (
                uri text, cid text, creator text, text text, created_at text, indexed_at text
            )".into(),
            "uri, cid, creator, text, created_at, indexed_at".into(),
            "INSERT INTO post (uri, cid, creator, text, \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, text, created_at, indexed_at FROM _merge_post
             ORDER BY uri
             ON CONFLICT DO NOTHING".into(),
        ),
        "like" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_like (
                uri text, cid text, creator text, subject text, subject_cid text, created_at text, indexed_at text
            )".into(),
            "uri, cid, creator, subject, subject_cid, created_at, indexed_at".into(),
            "INSERT INTO \"like\" (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, subject_cid, created_at, indexed_at FROM _merge_like
             ORDER BY uri
             ON CONFLICT DO NOTHING".into(),
        ),
        "follow" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_follow (
                uri text, cid text, creator text, subject_did text, created_at text, indexed_at text
            )".into(),
            "uri, cid, creator, subject_did, created_at, indexed_at".into(),
            "INSERT INTO follow (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject_did, created_at, indexed_at FROM _merge_follow
             ORDER BY uri
             ON CONFLICT DO NOTHING".into(),
        ),
        "repost" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_repost (
                uri text, cid text, creator text, subject text, subject_cid text, created_at text, indexed_at text
            )".into(),
            "uri, cid, creator, subject, subject_cid, created_at, indexed_at".into(),
            "INSERT INTO repost (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, subject_cid, created_at, indexed_at FROM _merge_repost
             ORDER BY uri
             ON CONFLICT DO NOTHING".into(),
        ),
        "feed_item" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_feed_item (
                type text, uri text, cid text, post_uri text, originator_did text, sort_at text
            )".into(),
            "type, uri, cid, post_uri, originator_did, sort_at".into(),
            "INSERT INTO feed_item (type, uri, cid, \"postUri\", \"originatorDid\", \"sortAt\")
             SELECT type, uri, cid, post_uri, originator_did, sort_at FROM _merge_feed_item
             ORDER BY uri
             ON CONFLICT DO NOTHING".into(),
        ),
        "block" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_block (
                uri text, cid text, creator text, subject text, created_at text, indexed_at text
            )".into(),
            "uri, cid, creator, subject, created_at, indexed_at".into(),
            "INSERT INTO actor_block (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
             SELECT uri, cid, creator, subject, created_at, indexed_at FROM _merge_block
             ORDER BY uri
             ON CONFLICT DO NOTHING".into(),
        ),
        "profile" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_profile (
                uri text, cid text, creator text, display_name text, description text, avatar_cid text, banner_cid text, indexed_at text
            )".into(),
            "uri, cid, creator, display_name, description, avatar_cid, banner_cid, indexed_at".into(),
            "INSERT INTO profile (uri, cid, creator, \"displayName\", description, \"avatarCid\", \"bannerCid\", \"indexedAt\")
             SELECT uri, cid, creator, display_name, description, avatar_cid, banner_cid, indexed_at FROM _merge_profile
             ORDER BY uri
             ON CONFLICT (uri) DO UPDATE SET
               cid = EXCLUDED.cid,
               \"displayName\" = EXCLUDED.\"displayName\",
               description = EXCLUDED.description,
               \"avatarCid\" = EXCLUDED.\"avatarCid\",
               \"bannerCid\" = EXCLUDED.\"bannerCid\",
               \"indexedAt\" = EXCLUDED.\"indexedAt\"".into(),
        ),
        "post_embed_image" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_post_embed_image (
                post_uri text, position text, image_cid text, alt text
            )".into(),
            "post_uri, position, image_cid, alt".into(),
            "INSERT INTO post_embed_image (\"postUri\", position, \"imageCid\", alt)
             SELECT post_uri, position, image_cid, alt FROM _merge_post_embed_image
             ORDER BY post_uri
             ON CONFLICT DO NOTHING".into(),
        ),
        "post_embed_video" => (
            "CREATE TEMP TABLE IF NOT EXISTS _merge_post_embed_video (
                post_uri text, video_cid text, alt text
            )".into(),
            "post_uri, video_cid, alt".into(),
            "INSERT INTO post_embed_video (\"postUri\", \"videoCid\", alt)
             SELECT post_uri, video_cid, alt FROM _merge_post_embed_video
             ORDER BY post_uri
             ON CONFLICT DO NOTHING".into(),
        ),
        other => panic!("unknown table: {other}"),
    }
}
