use clap::{Parser, Subcommand};
use color_eyre::eyre::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(
    name = "palomar-sync",
    about = "Sync followersFuzzy and PageRank to OpenSearch"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sync followersFuzzy from profile_agg to OpenSearch
    Followers,
    /// Export follow graph, compute PageRank, and index results to OpenSearch
    Pagerank {
        /// Path to the pagerank binary
        #[arg(long, env = "PAGERANK_BIN", default_value = "/usr/local/bin/pagerank")]
        pagerank_bin: PathBuf,

        /// Directory for temporary CSV files
        #[arg(long, env = "WORK_DIR", default_value = "/data")]
        work_dir: PathBuf,
    },
}

struct Config {
    database_url: String,
    opensearch_url: String,
    profile_index: String,
    batch_size: usize,
}

impl Config {
    fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?,
            opensearch_url: std::env::var("OPENSEARCH_URL")
                .unwrap_or_else(|_| "http://localhost:9200".to_string()),
            profile_index: std::env::var("PROFILE_INDEX")
                .unwrap_or_else(|_| "palomar_profile".to_string()),
            batch_size: std::env::var("BATCH_SIZE")
                .unwrap_or_else(|_| "5000".to_string())
                .parse()
                .unwrap_or(5000),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Install default rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let cli = Cli::parse();
    let config = Config::from_env()?;

    match cli.command {
        Command::Followers => sync_followers(&config).await,
        Command::Pagerank {
            pagerank_bin,
            work_dir,
        } => sync_pagerank(&config, &pagerank_bin, &work_dir).await,
    }
}

async fn connect_pg(database_url: &str) -> Result<tokio_postgres::Client> {
    let (client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
        .await
        .wrap_err("failed to connect to PostgreSQL")?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            error!(error = %e, "PostgreSQL connection error");
        }
    });

    Ok(client)
}

/// Bulk-update OpenSearch documents via the Bulk API.
/// Each entry in `updates` is (doc_id, update_body_json).
async fn bulk_update(
    http: &Client,
    opensearch_url: &str,
    index: &str,
    updates: &[(String, serde_json::Value)],
) -> Result<usize> {
    if updates.is_empty() {
        return Ok(0);
    }

    let mut body = String::with_capacity(updates.len() * 200);
    for (doc_id, update_body) in updates {
        let meta = json!({"update": {"_id": doc_id}});
        body.push_str(&meta.to_string());
        body.push('\n');
        body.push_str(&update_body.to_string());
        body.push('\n');
    }

    let url = format!("{}/{}/_bulk", opensearch_url, index);
    let resp = http
        .post(&url)
        .header("Content-Type", "application/x-ndjson")
        .body(body)
        .send()
        .await
        .wrap_err("failed to send bulk request to OpenSearch")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        error!(status = %status, body = %text, "OpenSearch bulk request failed");
        return Err(color_eyre::eyre::eyre!(
            "OpenSearch bulk request failed: {}",
            status
        ));
    }

    let resp_body: serde_json::Value = resp
        .json()
        .await
        .wrap_err("failed to parse bulk response")?;

    if resp_body["errors"].as_bool().unwrap_or(false) {
        let items = resp_body["items"].as_array();
        let error_count = items
            .map(|items| {
                items
                    .iter()
                    .filter(|item| item["update"]["error"].is_object())
                    .count()
            })
            .unwrap_or(0);
        if error_count > 0 {
            warn!(error_count, "some bulk update items had errors");
        }
    }

    Ok(updates.len())
}

/// Keyset-paginated followersFuzzy sync. Fetches batch_size rows at a time
/// ordered by did, using did > $last_did to avoid statement timeouts on large tables.
async fn sync_followers(config: &Config) -> Result<()> {
    info!("starting followersFuzzy sync from profile_agg to OpenSearch");

    let pg = connect_pg(&config.database_url).await?;
    let http = Client::new();

    let mut last_did = String::new();
    let mut indexed = 0usize;
    let page_size: i64 = config.batch_size as i64;
    let start = std::time::Instant::now();

    loop {
        let rows = pg
            .query(
                r#"SELECT did, "followersCount" FROM bsky.profile_agg
                   WHERE "followersCount" > 0 AND did > $1
                   ORDER BY did
                   LIMIT $2"#,
                &[&last_did, &page_size],
            )
            .await
            .wrap_err("failed to query profile_agg")?;

        if rows.is_empty() {
            break;
        }

        let mut batch: Vec<(String, serde_json::Value)> = Vec::with_capacity(rows.len());
        for row in &rows {
            let did: &str = row.get(0);
            let count: i32 = row.get(1);
            last_did = did.to_string();

            let update_body = json!({
                "script": {
                    "source": "ctx._source.followersFuzzy = params.f",
                    "lang": "painless",
                    "params": { "f": count }
                }
            });
            batch.push((did.to_string(), update_body));
        }

        let n = bulk_update(&http, &config.opensearch_url, &config.profile_index, &batch).await?;
        indexed += n;
        info!(
            indexed,
            last_did,
            elapsed_secs = start.elapsed().as_secs(),
            "followersFuzzy sync progress"
        );
    }

    info!(
        indexed,
        elapsed_secs = start.elapsed().as_secs(),
        "followersFuzzy sync complete"
    );
    Ok(())
}

/// Export follow graph and actor list using psql COPY for streaming large tables,
/// then run the external pagerank binary and index results to OpenSearch.
async fn sync_pagerank(config: &Config, pagerank_bin: &Path, work_dir: &Path) -> Result<()> {
    info!("starting PageRank pipeline");

    let follows_file = work_dir.join("follows.csv");
    let actors_file = work_dir.join("actors.csv");
    let output_file = work_dir.join("pageranks.csv");

    // Parse database URL to extract connection params for psql
    let db_url = &config.database_url;

    // Step 1: Export follow edges via psql COPY (streams, no memory issues)
    info!("exporting follow graph to CSV via psql COPY");
    let export_start = std::time::Instant::now();

    let copy_follows_sql =
        r#"COPY (SELECT "creator", "subjectDid" FROM bsky.follow) TO STDOUT WITH CSV"#;

    let follows_status = tokio::process::Command::new("psql")
        .arg(db_url)
        .arg("-c")
        .arg(copy_follows_sql)
        .stdout(Stdio::from(
            std::fs::File::create(&follows_file).wrap_err("failed to create follows CSV")?,
        ))
        .stderr(Stdio::inherit())
        .status()
        .await
        .wrap_err("failed to run psql for follows export")?;

    if !follows_status.success() {
        return Err(color_eyre::eyre::eyre!(
            "psql follows export failed with status: {}",
            follows_status
        ));
    }

    info!(
        elapsed_secs = export_start.elapsed().as_secs(),
        "follow graph CSV export complete"
    );

    // Step 2: Export actor DIDs via psql COPY
    info!("exporting actor DIDs to CSV via psql COPY");

    let copy_actors_sql = "COPY (SELECT did FROM bsky.actor) TO STDOUT WITH CSV";

    let actors_status = tokio::process::Command::new("psql")
        .arg(db_url)
        .arg("-c")
        .arg(copy_actors_sql)
        .stdout(Stdio::from(
            std::fs::File::create(&actors_file).wrap_err("failed to create actors CSV")?,
        ))
        .stderr(Stdio::inherit())
        .status()
        .await
        .wrap_err("failed to run psql for actors export")?;

    if !actors_status.success() {
        return Err(color_eyre::eyre::eyre!(
            "psql actors export failed with status: {}",
            actors_status
        ));
    }

    // Count actors for the pagerank binary
    let actor_count = count_lines(&actors_file).await?;
    info!(actor_count, "actor CSV export complete");

    // Step 3: Run pagerank binary
    info!(
        bin = %pagerank_bin.display(),
        follows = %follows_file.display(),
        actors = %actors_file.display(),
        output = %output_file.display(),
        "running pagerank binary"
    );

    let status = tokio::process::Command::new(pagerank_bin)
        .env("FOLLOWS_FILE", &follows_file)
        .env("ACTORS_FILE", &actors_file)
        .env("OUTPUT_FILE", &output_file)
        .env("EXPECTED_ACTOR_COUNT", actor_count.to_string())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .wrap_err("failed to spawn pagerank binary")?;

    if !status.success() {
        return Err(color_eyre::eyre::eyre!(
            "pagerank binary exited with status: {}",
            status
        ));
    }

    info!("pagerank computation complete, indexing results");

    // Step 4: Stream output CSV and bulk-index to OpenSearch
    let file = tokio::fs::File::open(&output_file)
        .await
        .wrap_err("failed to open pagerank output CSV")?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let http = Client::new();
    let mut batch: Vec<(String, serde_json::Value)> = Vec::with_capacity(config.batch_size);
    let mut indexed = 0usize;
    let mut line_count = 0usize;
    let index_start = std::time::Instant::now();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let Some((did, rank_str)) = line.split_once(',') else {
            warn!(line, "skipping malformed pagerank line");
            continue;
        };

        let rank: f64 = match rank_str.parse() {
            Ok(r) => r,
            Err(e) => {
                warn!(did, rank_str, error = %e, "skipping unparseable rank");
                continue;
            }
        };

        line_count += 1;
        let update_body = json!({
            "script": {
                "source": "ctx._source.pagerank = params.pagerank",
                "lang": "painless",
                "params": { "pagerank": rank }
            }
        });

        batch.push((did.to_string(), update_body));

        if batch.len() >= config.batch_size {
            let n =
                bulk_update(&http, &config.opensearch_url, &config.profile_index, &batch).await?;
            indexed += n;
            info!(
                indexed,
                line_count,
                elapsed_secs = index_start.elapsed().as_secs(),
                "pagerank indexing progress"
            );
            batch.clear();
        }
    }

    // Flush remaining
    if !batch.is_empty() {
        let n = bulk_update(&http, &config.opensearch_url, &config.profile_index, &batch).await?;
        indexed += n;
    }

    info!(
        indexed,
        line_count,
        elapsed_secs = index_start.elapsed().as_secs(),
        "pagerank indexing complete"
    );

    // Cleanup temp files
    for path in [&follows_file, &actors_file, &output_file] {
        if let Err(e) = tokio::fs::remove_file(path).await {
            warn!(path = %path.display(), error = %e, "failed to remove temp file");
        }
    }

    info!("PageRank pipeline complete");
    Ok(())
}

async fn count_lines(path: &Path) -> Result<usize> {
    let file = tokio::fs::File::open(path)
        .await
        .wrap_err("failed to open file for line count")?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut count = 0usize;
    while lines.next_line().await?.is_some() {
        count += 1;
    }
    Ok(count)
}
