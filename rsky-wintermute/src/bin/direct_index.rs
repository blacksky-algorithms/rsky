use std::io::Cursor;
use std::sync::Arc;

use clap::Parser;
use color_eyre::Result;
use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
use iroh_car::CarReader;
use rsky_identity::types::IdentityResolverOpts;
use rsky_identity::IdResolver;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_repo::readable_repo::ReadableRepo;
use rsky_syntax::aturi::AtUri;
use tokio_postgres::NoTls;

use rsky_repo::parse::get_and_parse_record;
use rsky_wintermute::backfiller::convert_record_to_ipld;
use rsky_wintermute::indexer::IndexerManager;
use rsky_wintermute::types::{IndexJob, WriteAction};

#[derive(Debug, Parser)]
#[command(name = "direct_index")]
#[command(about = "Directly fetch and index a repo, bypassing queues")]
struct Args {
    /// DIDs to index (comma-separated or multiple --did flags)
    #[arg(long = "did", num_args = 1..)]
    dids: Vec<String>,

    /// PostgreSQL connection URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Parse all DIDs from args (supporting comma-separated)
    let dids: Vec<String> = args
        .dids
        .iter()
        .flat_map(|d| d.split(',').map(|s| s.trim().to_string()))
        .filter(|d| !d.is_empty() && d.starts_with("did:"))
        .collect();

    if dids.is_empty() {
        eprintln!("No valid DIDs provided");
        return Ok(());
    }

    println!("Will index {} DIDs directly to PostgreSQL", dids.len());

    // Setup database pool
    let mut cfg = Config::new();
    cfg.url = Some(args.database_url.clone());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });

    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)?;
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    for did in &dids {
        println!("\n=== Processing {} ===", did);
        match process_did(&pool, &http_client, did).await {
            Ok(count) => println!("Successfully indexed {} records for {}", count, did),
            Err(e) => eprintln!("Failed to index {}: {}", did, e),
        }
    }

    println!("\nDone!");
    Ok(())
}

async fn process_did(
    pool: &deadpool_postgres::Pool,
    http_client: &reqwest::Client,
    did: &str,
) -> Result<usize> {
    // Resolve DID to get PDS endpoint
    let resolver_opts = IdentityResolverOpts {
        timeout: None,
        plc_url: None,
        did_cache: None,
        backup_nameservers: None,
    };
    let mut resolver = IdResolver::new(resolver_opts);
    let doc = resolver
        .did
        .resolve(did.to_string(), None)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("DID resolution error: {}", e))?
        .ok_or_else(|| color_eyre::eyre::eyre!("DID resolution failed"))?;

    let mut pds_endpoint = None;
    if let Some(services) = &doc.service {
        for service in services {
            if service.r#type == "AtprotoPersonalDataServer" || service.id == "#atproto_pds" {
                pds_endpoint = Some(service.service_endpoint.clone());
                break;
            }
        }
    }

    let pds_endpoint =
        pds_endpoint.ok_or_else(|| color_eyre::eyre::eyre!("No PDS endpoint found"))?;

    println!("  PDS: {}", pds_endpoint);

    // Fetch CAR file
    let repo_url = format!("{pds_endpoint}/xrpc/com.atproto.sync.getRepo?did={did}");
    println!("  Fetching CAR...");
    let response = http_client.get(&repo_url).send().await?;

    if !response.status().is_success() {
        return Err(color_eyre::eyre::eyre!("HTTP error: {}", response.status()));
    }

    let car_bytes = response.bytes().await?;
    println!("  CAR size: {} bytes", car_bytes.len());

    // Parse CAR file
    let mut reader = CarReader::new(Cursor::new(car_bytes.to_vec()))
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to parse CAR file: {}", e))?;
    let root = *reader
        .header()
        .roots()
        .first()
        .ok_or_else(|| color_eyre::eyre::eyre!("No root CID"))?;

    let mut blocks = rsky_repo::block_map::BlockMap::new();
    while let Some((cid, data)) = reader
        .next_block()
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to read block: {}", e))?
    {
        blocks.set(cid, data.clone());
    }

    let blockstore = MemoryBlockstore::new(Some(blocks))
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to create blockstore: {}", e))?;
    let storage_arc = Arc::new(tokio::sync::RwLock::new(blockstore));

    let mut repo = ReadableRepo::load(storage_arc, root)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to load repo: {}", e))?;

    if repo.did() != did {
        return Err(color_eyre::eyre::eyre!(
            "DID mismatch: expected {}, got {}",
            did,
            repo.did()
        ));
    }

    // Get all records
    let leaves = repo
        .data
        .list(None, None, None)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to list records: {}", e))?;
    println!("  Found {} records", leaves.len());

    let blocks_result = {
        let storage_guard = repo.storage.read().await;
        storage_guard
            .get_blocks(leaves.iter().map(|e| e.value).collect())
            .await
            .map_err(|e| color_eyre::eyre::eyre!("Failed to get blocks: {}", e))?
    };

    let rev = repo.commit.rev.clone();
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();

    let mut indexed_count = 0;
    let mut skipped_count = 0;

    for entry in &leaves {
        let uri_string = format!("at://{did}/{}", entry.key);
        let Ok(uri) = AtUri::new(uri_string, None) else {
            skipped_count += 1;
            continue;
        };

        let collection = uri.get_collection();
        let rkey = uri.get_rkey();

        // Filter to bsky/chat records
        if !collection.starts_with("app.bsky.") && !collection.starts_with("chat.bsky.") {
            skipped_count += 1;
            continue;
        }

        if let Ok(parsed) = get_and_parse_record(&blocks_result.blocks, entry.value) {
            let record_json_raw = serde_json::to_value(&parsed.record)?;
            let record_json = convert_record_to_ipld(&record_json_raw);

            let uri_string = format!("at://{did}/{collection}/{rkey}");
            let uri = AtUri::new(uri_string.clone(), None)
                .map_err(|e| color_eyre::eyre::eyre!("Invalid URI {}: {}", uri_string, e))?;
            let cid = entry.value.to_string();

            let job = IndexJob {
                uri: uri.to_string(),
                cid,
                action: WriteAction::Create,
                record: Some(record_json),
                indexed_at: now.clone(),
                rev: rev.clone(),
            };

            // Index directly to PostgreSQL
            if let Err(e) = IndexerManager::process_job(pool, &job).await {
                eprintln!("  Warning: failed to index {}: {}", job.uri, e);
            } else {
                indexed_count += 1;
            }
        } else {
            skipped_count += 1;
        }

        if indexed_count > 0 && indexed_count % 100 == 0 {
            print!("\r  Indexed {} records...", indexed_count);
        }
    }

    println!("\r  Indexed: {}, Skipped: {}", indexed_count, skipped_count);

    // Update profile_agg
    println!("  Updating profile aggregates...");
    let client = pool.get().await?;
    client
        .execute(
            "INSERT INTO profile_agg (did, \"postsCount\")
             SELECT $1, COUNT(*) FROM post WHERE creator = $1
             ON CONFLICT (did) DO UPDATE SET \"postsCount\" = EXCLUDED.\"postsCount\"",
            &[&did],
        )
        .await?;

    client
        .execute(
            "INSERT INTO profile_agg (did, \"followsCount\")
             SELECT $1, COUNT(*) FROM follow WHERE creator = $1
             ON CONFLICT (did) DO UPDATE SET \"followsCount\" = EXCLUDED.\"followsCount\"",
            &[&did],
        )
        .await?;

    client
        .execute(
            "INSERT INTO profile_agg (did, \"followersCount\")
             SELECT $1, COUNT(*) FROM follow WHERE \"subjectDid\" = $1
             ON CONFLICT (did) DO UPDATE SET \"followersCount\" = EXCLUDED.\"followersCount\"",
            &[&did],
        )
        .await?;

    Ok(indexed_count)
}
