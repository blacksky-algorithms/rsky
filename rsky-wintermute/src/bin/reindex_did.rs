//! Reconciles one actor's downstream state against its repository, or
//! verifies that a recovery commit was acknowledged.

use clap::Parser;
use color_eyre::Result;
use rsky_wintermute::reconcile::frontier::FrontierClient;
use rsky_wintermute::reconcile::workflow::{ReconcileOptions, reconcile, verify_recovery};

#[derive(Debug, Parser)]
#[command(name = "reindex_did")]
struct Args {
    #[arg(long)]
    did: String,
    /// Run the fenced reconciliation.
    #[arg(long)]
    reconcile: bool,
    /// Override downstream state newer than the repository.
    #[arg(long)]
    reset_to_repo: bool,
    /// Reconcile against an incomplete history; the obligation never converges.
    #[arg(long)]
    break_glass: bool,
    /// Report without fencing or writing.
    #[arg(long)]
    dry_run: bool,
    /// Verify that this recovery commit was acknowledged downstream.
    #[arg(long)]
    verify_recovery: Option<String>,
    /// Names this run in the fence and the journal.
    #[arg(long)]
    workflow_id: Option<String>,
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
    /// The PDS answering frontier and export reads, as a fixed address.
    #[arg(long, env = "RECONCILE_PDS_URL")]
    pds_url: Option<String>,
    #[arg(long, env = "RECONCILE_PDS_ADMIN_PASSWORD")]
    pds_admin_password: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    color_eyre::install()?;
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| color_eyre::eyre::eyre!("crypto provider already installed"))?;
    let args = Args::parse();
    let pool = rsky_wintermute::config::create_pg_pool(
        &args.database_url,
        rsky_wintermute::config::pg_pool_config(8),
    )?;

    if let Some(commit_cid) = &args.verify_recovery {
        let verification = verify_recovery(&pool, &args.did, commit_cid).await?;
        println!("{}", serde_json::to_string_pretty(&verification)?);
        std::process::exit(if verification.acknowledged { 0 } else { 1 });
    }
    if !args.reconcile {
        return Err(color_eyre::eyre::eyre!(
            "nothing to do: pass --reconcile or --verify-recovery <cid>"
        ));
    }
    let pds_url = args
        .pds_url
        .ok_or_else(|| color_eyre::eyre::eyre!("RECONCILE_PDS_URL is required"))?;
    let password = args
        .pds_admin_password
        .ok_or_else(|| color_eyre::eyre::eyre!("RECONCILE_PDS_ADMIN_PASSWORD is required"))?;
    let pds = FrontierClient::new(&pds_url, &password)?;
    let workflow_id = args.workflow_id.unwrap_or_else(|| {
        format!(
            "reconcile-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
        )
    });
    let report = reconcile(
        &pool,
        &pds,
        &ReconcileOptions {
            did: args.did.clone(),
            workflow_id,
            reset_to_repo: args.reset_to_repo,
            break_glass: args.break_glass,
            dry_run: args.dry_run,
        },
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    std::process::exit(if report.refusal.is_some() { 1 } else { 0 });
}
