//! Moves existing accounts off the shared repo signing key onto their own.
//!
//! Publishes a PLC operation and emits firehose events per account, so it is
//! never run automatically — an operator runs it deliberately. It is
//! resumable: a DID whose document already names its own key is skipped.
//! Run it against a quiesced PDS; the per-DID write lock is process-local.
//!
//!     rotate-keys [--dry-run] [--did <did>]...

use anyhow::{bail, Result};
use rsky_pds::account_manager::AccountManager;
use rsky_pds::actor_store::blobstore::BlobstoreFactory;
use rsky_pds::actor_store::ActorStore;
use rsky_pds::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;
use rsky_pds::background::BackgroundQueue;
use rsky_pds::config::env_to_cfg;
use rsky_pds::context::PDS_REPO_SIGNING_KEYPAIR;
use rsky_pds::crawlers::Crawlers;
use rsky_pds::rotate_keys::{rotate_keys, RotateKeysContext, RotateKeysOpts};
use rsky_pds::sequencer::Sequencer;
use rsky_pds::{account_manager, plc, sequencer};
use std::env;
use tokio::sync::RwLock;

const USAGE: &str = "usage: rotate-keys [--dry-run] [--did <did>]...";

#[derive(Debug)]
struct Args {
    dids: Option<Vec<String>>,
    dry_run: bool,
    help: bool,
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut dids: Vec<String> = Vec::new();
    let mut dry_run = false;
    let mut help = false;
    let mut argv = argv.into_iter();
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--did" => match argv.next() {
                Some(did) => dids.push(did),
                None => bail!("--did requires a value"),
            },
            "--help" | "-h" => help = true,
            other => bail!("unrecognised argument: {other}"),
        }
    }
    Ok(Args {
        dids: if dids.is_empty() { None } else { Some(dids) },
        dry_run,
        help,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::new())?;

    let args = parse_args(env::args().skip(1))?;
    if args.help {
        tracing::info!("{USAGE}");
        return Ok(());
    }

    // fail on a missing key here rather than part-way through a sweep
    let shared_signing_key = *PDS_REPO_SIGNING_KEYPAIR;
    let plc_rotation_key = PDS_PLC_ROTATION_KEYPAIR.secret_key();

    let cfg = env_to_cfg();
    let account_manager = AccountManager::new(
        account_manager::db::get_migrated_db(&cfg.service_db.account_db_location).await?,
    );
    let sequencer = RwLock::new(Sequencer::new(
        sequencer::db::get_migrated_db(&cfg.service_db.sequencer_db_location).await?,
        Crawlers::new(cfg.service.hostname.clone(), cfg.crawlers.clone()),
        None,
    ));
    let actor_store = ActorStore::new(&cfg.actor_store, BackgroundQueue::default());
    let aws_sdk_config = aws_config::from_env()
        .endpoint_url(env::var("AWS_ENDPOINT").unwrap_or("localhost".to_owned()))
        .load()
        .await;
    let blobstore_factory = BlobstoreFactory::new(cfg.blobstore.clone(), aws_sdk_config);
    let plc_client = plc::Client::new(cfg.identity.plc_url.clone());

    let report = rotate_keys(
        &RotateKeysContext {
            actor_store: &actor_store,
            account_manager: &account_manager,
            blobstore_factory: &blobstore_factory,
            sequencer: &sequencer,
            plc_client: &plc_client,
            plc_rotation_key: &plc_rotation_key,
            shared_signing_key: &shared_signing_key,
        },
        RotateKeysOpts {
            dids: args.dids,
            dry_run: args.dry_run,
        },
    )
    .await?;
    tracing::info!(
        scanned = report.scanned,
        rotated = report.rotated,
        skipped = report.skipped,
        failed = report.failed,
        dry_run = args.dry_run,
        "signing key rotation finished"
    );
    if report.failed > 0 {
        bail!("{} account(s) failed to rotate", report.failed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(argv: &[&str]) -> Args {
        parse_args(argv.iter().map(|arg| (*arg).to_owned())).unwrap()
    }

    #[test]
    fn no_arguments_sweeps_every_account() {
        let parsed = args(&[]);
        assert!(parsed.dids.is_none());
        assert!(!parsed.dry_run);
        assert!(!parsed.help);
    }

    #[test]
    fn repeated_did_flags_accumulate() {
        let parsed = args(&[
            "--did",
            "did:plc:alice",
            "--dry-run",
            "--did",
            "did:plc:bob",
        ]);
        assert_eq!(
            parsed.dids,
            Some(vec!["did:plc:alice".to_owned(), "did:plc:bob".to_owned()])
        );
        assert!(parsed.dry_run);
    }

    #[test]
    fn help_is_recognised() {
        assert!(args(&["-h"]).help);
        assert!(args(&["--help"]).help);
    }

    #[test]
    fn a_did_flag_without_a_value_is_rejected() {
        let error = parse_args(["--did".to_owned()]).unwrap_err().to_string();
        assert_eq!(error, "--did requires a value");
    }

    #[test]
    fn an_unknown_flag_is_rejected() {
        let error = parse_args(["--nope".to_owned()]).unwrap_err().to_string();
        assert_eq!(error, "unrecognised argument: --nope");
    }
}
