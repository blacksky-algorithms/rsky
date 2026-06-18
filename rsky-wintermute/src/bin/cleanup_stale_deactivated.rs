use std::sync::Arc;

use clap::Parser;
use color_eyre::Result;
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use futures::stream::{self, StreamExt};
use rsky_identity::IdResolver;
use rsky_identity::types::IdentityResolverOpts;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio_postgres::NoTls;
use tracing::{debug, info, warn};

#[derive(Debug, Parser)]
#[command(name = "cleanup_stale_deactivated")]
#[command(
    about = "Re-checks PDS state for actors marked upstreamStatus='deactivated' with NULL accountEventAt, and clears the flag for any that are actually active."
)]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    #[arg(long, default_value = "https://plc.directory")]
    plc_url: String,

    #[arg(long, default_value_t = 50)]
    concurrency: usize,

    #[arg(long, default_value_t = 5000)]
    pds_timeout_ms: u64,

    #[arg(long)]
    dry_run: bool,

    #[arg(long, default_value_t = 0)]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct RepoStatus {
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct Counters {
    recovered: u64,
    confirmed: u64,
    deferred_no_pds: u64,
    deferred_pds_err: u64,
    deferred_plc_err: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let mut cfg = Config::new();
    cfg.url = Some(args.database_url.clone());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = Arc::new(cfg.create_pool(Some(Runtime::Tokio1), NoTls)?);

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(args.pds_timeout_ms))
        .user_agent("rsky-wintermute-cleanup/0.1")
        .build()?;

    let dids = fetch_targets(&pool, args.limit).await?;
    info!("found {} actors to re-check", dids.len());

    if dids.is_empty() {
        return Ok(());
    }

    let counters = Arc::new(Mutex::new(Counters::default()));
    let processed = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let total = dids.len() as u64;

    let plc_url = Arc::new(args.plc_url.clone());
    let dry_run = args.dry_run;

    stream::iter(dids.into_iter())
        .for_each_concurrent(args.concurrency, |did| {
            let pool = pool.clone();
            let http = http.clone();
            let plc_url = plc_url.clone();
            let counters = counters.clone();
            let processed = processed.clone();
            async move {
                let outcome = process_one(&pool, &http, &plc_url, &did, dry_run).await;
                {
                    let mut c = counters.lock().await;
                    match outcome {
                        Outcome::Recovered => c.recovered += 1,
                        Outcome::Confirmed => c.confirmed += 1,
                        Outcome::DeferredNoPds => c.deferred_no_pds += 1,
                        Outcome::DeferredPdsErr => c.deferred_pds_err += 1,
                        Outcome::DeferredPlcErr => c.deferred_plc_err += 1,
                    }
                }
                let n = processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if n % 500 == 0 {
                    let c = counters.lock().await.clone();
                    info!(
                        "progress: {}/{} (recovered={}, confirmed={}, deferred={})",
                        n,
                        total,
                        c.recovered,
                        c.confirmed,
                        c.deferred_no_pds + c.deferred_pds_err + c.deferred_plc_err
                    );
                }
            }
        })
        .await;

    let c = counters.lock().await.clone();
    info!(
        "done: total={} recovered={} confirmed={} deferred_no_pds={} deferred_pds_err={} deferred_plc_err={}",
        total, c.recovered, c.confirmed, c.deferred_no_pds, c.deferred_pds_err, c.deferred_plc_err
    );

    Ok(())
}

enum Outcome {
    Recovered,
    Confirmed,
    DeferredNoPds,
    DeferredPdsErr,
    DeferredPlcErr,
}

async fn fetch_targets(pool: &Pool, limit: usize) -> Result<Vec<String>> {
    let client = pool.get().await?;
    let query = if limit > 0 {
        format!(
            "SELECT did FROM actor WHERE \"upstreamStatus\" = 'deactivated' AND \"accountEventAt\" IS NULL LIMIT {limit}"
        )
    } else {
        "SELECT did FROM actor WHERE \"upstreamStatus\" = 'deactivated' AND \"accountEventAt\" IS NULL"
            .to_owned()
    };
    let rows = client.query(query.as_str(), &[]).await?;
    Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
}

async fn process_one(
    pool: &Pool,
    http: &reqwest::Client,
    plc_url: &str,
    did: &str,
    dry_run: bool,
) -> Outcome {
    let pds = match resolve_pds(plc_url, did).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            debug!("{}: no PDS endpoint in DID doc", did);
            return Outcome::DeferredNoPds;
        }
        Err(e) => {
            debug!("{}: PLC error: {}", did, e);
            return Outcome::DeferredPlcErr;
        }
    };

    let url = format!(
        "{}/xrpc/com.atproto.sync.getRepoStatus?did={}",
        pds.trim_end_matches('/'),
        did
    );
    match http.get(&url).send().await {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                debug!("{}: getRepoStatus HTTP {}", did, status);
                return Outcome::DeferredPdsErr;
            }
            match resp.json::<RepoStatus>().await {
                Ok(rs) => apply_decision(pool, did, &rs, dry_run).await,
                Err(e) => {
                    debug!("{}: getRepoStatus parse error: {}", did, e);
                    Outcome::DeferredPdsErr
                }
            }
        }
        Err(e) => {
            debug!("{}: getRepoStatus request error: {}", did, e);
            Outcome::DeferredPdsErr
        }
    }
}

async fn apply_decision(pool: &Pool, did: &str, status: &RepoStatus, dry_run: bool) -> Outcome {
    let active = status.active.unwrap_or(false);
    if active {
        if dry_run {
            info!("[dry-run] would recover {}", did);
        } else if let Err(e) = clear_status(pool, did).await {
            warn!("{}: clear_status failed: {}", did, e);
            return Outcome::DeferredPdsErr;
        }
        Outcome::Recovered
    } else {
        if dry_run {
            info!(
                "[dry-run] would confirm {} (active=false status={:?})",
                did, status.status
            );
        } else if let Err(e) = mark_event_at_now(pool, did).await {
            warn!("{}: mark_event_at_now failed: {}", did, e);
            return Outcome::DeferredPdsErr;
        }
        Outcome::Confirmed
    }
}

async fn clear_status(pool: &Pool, did: &str) -> Result<()> {
    let client = pool.get().await?;
    client
        .execute(
            "UPDATE actor SET \"upstreamStatus\" = NULL, \"accountEventAt\" = NOW() WHERE did = $1 AND \"upstreamStatus\" = 'deactivated' AND \"accountEventAt\" IS NULL",
            &[&did],
        )
        .await?;
    Ok(())
}

async fn mark_event_at_now(pool: &Pool, did: &str) -> Result<()> {
    let client = pool.get().await?;
    client
        .execute(
            "UPDATE actor SET \"accountEventAt\" = NOW() WHERE did = $1 AND \"accountEventAt\" IS NULL",
            &[&did],
        )
        .await?;
    Ok(())
}

async fn resolve_pds(plc_url: &str, did: &str) -> Result<Option<String>> {
    let mut resolver = IdResolver::new(IdentityResolverOpts {
        timeout: Some(std::time::Duration::from_secs(5)),
        plc_url: Some(plc_url.to_owned()),
        did_cache: None,
        backup_nameservers: None,
    });
    match resolver.did.resolve(did.to_owned(), None).await {
        Ok(Some(doc)) => Ok(doc.service.as_ref().and_then(|svcs| {
            svcs.iter()
                .find(|s| s.id == "#atproto_pds")
                .map(|s| s.service_endpoint.clone())
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(color_eyre::eyre::eyre!("PLC resolve failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_decision_active_marks_recovered() {
        let status = RepoStatus {
            active: Some(true),
            status: None,
        };
        assert!(status.active.unwrap_or(false));
    }

    #[test]
    fn apply_decision_inactive_with_status() {
        let status = RepoStatus {
            active: Some(false),
            status: Some("deactivated".to_owned()),
        };
        assert!(!status.active.unwrap_or(false));
        assert_eq!(status.status.as_deref(), Some("deactivated"));
    }

    #[test]
    fn apply_decision_missing_active_defaults_inactive() {
        let status = RepoStatus {
            active: None,
            status: None,
        };
        assert!(!status.active.unwrap_or(false));
    }

    #[test]
    fn parse_repo_status_typical() {
        let json = r#"{"did":"did:plc:abc","active":true,"rev":"3xx"}"#;
        let rs: RepoStatus = serde_json::from_str(json).unwrap();
        assert_eq!(rs.active, Some(true));
    }

    #[test]
    fn parse_repo_status_deactivated() {
        let json = r#"{"did":"did:plc:abc","active":false,"status":"deactivated"}"#;
        let rs: RepoStatus = serde_json::from_str(json).unwrap();
        assert_eq!(rs.active, Some(false));
        assert_eq!(rs.status.as_deref(), Some("deactivated"));
    }

    #[test]
    fn parse_repo_status_missing_fields() {
        let json = r#"{}"#;
        let rs: RepoStatus = serde_json::from_str(json).unwrap();
        assert_eq!(rs.active, None);
        assert_eq!(rs.status, None);
    }
}
