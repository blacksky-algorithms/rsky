//! The binary's maintenance modes. Without arguments the server runs;
//! with one of these it performs one operator action over the data
//! directory named by the environment, prints a JSON result, and exits.

use crate::actor_store::blobstore::{BlobStore, BlobstoreFactory};
use crate::actor_store::ActorStore;
use crate::admission::Admission;
use crate::background::BackgroundQueue;
use crate::blob_attempts::AttemptJournal;
use crate::config::env_to_cfg;
use crate::crawlers::Crawlers;
use crate::drain::{drain_did, DrainStatus};
use crate::lifecycle::LifecycleStore;
use crate::locks::LockDir;
use crate::repair::{
    close_quarantine, quarantine_seq, run_repair, ExternalOutcome, RepairContext, RepairKind,
    RepairStore,
};
use crate::sequencer::Sequencer;
use crate::SharedSequencer;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

pub const USAGE: &str = "usage: rsky-pds [<maintenance command>]\n\
  --drain-did <did> [--timeout-secs <n>]\n\
  --repair-create <file.json>          {\"id\",\"did\",\"kind\":{\"kind\":\"republish\",\"uri\":...}}\n\
  --repair-run <id> [--timeout-secs <n>]\n\
  --repair-status <id>\n\
  --quarantine-open <seq> --did <did> --kind <kind> [--repair <id>]...\n\
  --quarantine-local-reconciled <seq>\n\
  --quarantine-close <seq> --external verified|accepted [--justification <text>]\n\
  --quarantine-status <seq>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Drain {
        did: String,
        timeout: Duration,
    },
    RepairCreate {
        file: PathBuf,
    },
    RepairRun {
        id: String,
        timeout: Duration,
    },
    RepairStatus {
        id: String,
    },
    QuarantineOpen {
        seq: i64,
        did: String,
        kind: String,
        repairs: Vec<String>,
    },
    QuarantineLocalReconciled {
        seq: i64,
    },
    QuarantineClose {
        seq: i64,
        external: ExternalOutcome,
        justification: Option<String>,
    },
    QuarantineStatus {
        seq: i64,
    },
}

/// `None` means run the server.
pub fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Option<Command>> {
    let args: Vec<String> = argv.into_iter().collect();
    if args.is_empty() {
        return Ok(None);
    }
    let value = |flag: &str| -> Result<Option<String>> {
        match args.iter().position(|arg| arg == flag) {
            None => Ok(None),
            Some(index) => match args.get(index + 1) {
                Some(value) if !value.starts_with("--") => Ok(Some(value.clone())),
                _ => bail!("{flag} requires a value\n{USAGE}"),
            },
        }
    };
    let values = |flag: &str| -> Vec<String> {
        args.iter()
            .enumerate()
            .filter(|(_, arg)| *arg == flag)
            .filter_map(|(index, _)| args.get(index + 1).cloned())
            .collect()
    };
    let seq_of = |flag: &str| -> Result<Option<i64>> {
        value(flag)?
            .map(|raw| {
                raw.parse::<i64>()
                    .with_context(|| format!("{flag} takes a sequence number"))
            })
            .transpose()
    };
    let timeout = match value("--timeout-secs")? {
        Some(raw) => Duration::from_secs(
            raw.parse::<u64>()
                .context("--timeout-secs takes a number")?,
        ),
        None => Duration::from_secs(600),
    };
    let known = [
        "--drain-did",
        "--repair-create",
        "--repair-run",
        "--repair-status",
        "--quarantine-open",
        "--quarantine-local-reconciled",
        "--quarantine-close",
        "--quarantine-status",
        "--timeout-secs",
        "--did",
        "--kind",
        "--repair",
        "--external",
        "--justification",
    ];
    if let Some(unknown) = args
        .iter()
        .filter(|arg| arg.starts_with("--"))
        .find(|arg| !known.contains(&arg.as_str()))
    {
        bail!("unrecognised argument: {unknown}\n{USAGE}");
    }
    let command = if let Some(did) = value("--drain-did")? {
        Command::Drain { did, timeout }
    } else if let Some(file) = value("--repair-create")? {
        Command::RepairCreate { file: file.into() }
    } else if let Some(id) = value("--repair-run")? {
        Command::RepairRun { id, timeout }
    } else if let Some(id) = value("--repair-status")? {
        Command::RepairStatus { id }
    } else if let Some(seq) = seq_of("--quarantine-open")? {
        let did = value("--did")?.context("--quarantine-open needs --did")?;
        let kind = value("--kind")?.context("--quarantine-open needs --kind")?;
        Command::QuarantineOpen {
            seq,
            did,
            kind,
            repairs: values("--repair"),
        }
    } else if let Some(seq) = seq_of("--quarantine-local-reconciled")? {
        Command::QuarantineLocalReconciled { seq }
    } else if let Some(seq) = seq_of("--quarantine-close")? {
        let external = match value("--external")?.as_deref() {
            Some("verified") => ExternalOutcome::Verified,
            Some("accepted") => ExternalOutcome::Accepted,
            _ => bail!("--quarantine-close needs --external verified|accepted"),
        };
        Command::QuarantineClose {
            seq,
            external,
            justification: value("--justification")?,
        }
    } else if let Some(seq) = seq_of("--quarantine-status")? {
        Command::QuarantineStatus { seq }
    } else {
        bail!("no command given\n{USAGE}");
    };
    Ok(Some(command))
}

/// The service's journals and stores, opened over the environment's data
/// directory without starting the server.
pub struct Maintenance {
    pub actor_store: ActorStore,
    pub sequencer: SharedSequencer,
    pub lifecycle: LifecycleStore,
    pub repairs: RepairStore,
    pub lock_dir: LockDir,
    blobstores: BlobstoreFactory,
}

impl Maintenance {
    pub async fn from_env() -> Result<Self> {
        let cfg = env_to_cfg();
        let lifecycle = LifecycleStore::open(&cfg.service_db.lifecycle_db_location).await?;
        let admission = Arc::new(match &cfg.service.write_allowlist_file {
            Some(path) => Admission::from_file(path)?,
            None => Admission::unrestricted(),
        });
        let lock_dir = LockDir::new(&cfg.service_db.lock_dir)?;
        let actor_store = ActorStore::new(
            &cfg.actor_store,
            BackgroundQueue::default(),
            lifecycle.clone(),
        )
        .with_coexistence(cfg.service.coexistence)
        .with_admission(admission)
        .with_lock_dir(lock_dir.clone());
        let sequencer = SharedSequencer {
            sequencer: RwLock::new(Sequencer::new(
                crate::sequencer::db::get_migrated_db(&cfg.service_db.sequencer_db_location)
                    .await?,
                Crawlers::new(cfg.service.hostname.clone(), vec![]),
                None,
            )),
        };
        let aws_sdk_config = aws_config::from_env()
            .endpoint_url(std::env::var("AWS_ENDPOINT").unwrap_or("localhost".to_owned()))
            .load()
            .await;
        let blobstores = BlobstoreFactory::new(cfg.blobstore.clone(), aws_sdk_config)
            .with_attempts(
                AttemptJournal::open(
                    &cfg.service_db.blob_attempts_db_location,
                    cfg.service.coexistence,
                )
                .await?,
            );
        let repairs = RepairStore::open(&cfg.service_db.repair_db_location).await?;
        Ok(Maintenance {
            actor_store,
            sequencer,
            lifecycle,
            repairs,
            lock_dir,
            blobstores,
        })
    }

    pub fn blobstore(&self, did: &str) -> Arc<dyn BlobStore> {
        self.blobstores.blobstore(did.to_owned())
    }

    fn repair_context(&self, did: &str) -> RepairContext<'_> {
        RepairContext {
            actor_store: &self.actor_store,
            sequencer: &self.sequencer,
            lifecycle: &self.lifecycle,
            repairs: &self.repairs,
            lock_dir: &self.lock_dir,
            blobstore: self.blobstore(did),
        }
    }

    pub async fn drain(&self, did: &str, timeout: Duration) -> Result<DrainStatus> {
        drain_did(
            &self.actor_store,
            &self.sequencer,
            &self.repairs,
            &self.lock_dir,
            self.blobstore(did),
            did,
            timeout,
        )
        .await
    }
}

#[derive(Debug, Deserialize)]
struct RepairFile {
    id: String,
    did: String,
    kind: RepairKind,
}

/// The command's JSON result and the process exit code it warrants.
pub async fn run(command: Command) -> Result<(serde_json::Value, i32)> {
    let maintenance = Maintenance::from_env().await?;
    match command {
        Command::Drain { did, timeout } => {
            let status = maintenance.drain(&did, timeout).await?;
            let code = if status.fully_drained { 0 } else { 1 };
            Ok((serde_json::to_value(status)?, code))
        }
        Command::RepairCreate { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("cannot read {}", file.display()))?;
            let spec: RepairFile = serde_json::from_str(&text).context("repair file")?;
            maintenance
                .repairs
                .create(&spec.id, &spec.did, &spec.kind)
                .await?;
            let repair = maintenance.repairs.get(&spec.id).await?;
            Ok((serde_json::to_value(repair)?, 0))
        }
        Command::RepairRun { id, timeout } => {
            let did = match maintenance.repairs.get(&id).await? {
                Some(repair) => repair.did,
                None => bail!("no repair {id}"),
            };
            let repair = run_repair(&maintenance.repair_context(&did), &id, timeout, None).await?;
            let code = if repair.state == "done" { 0 } else { 1 };
            Ok((serde_json::to_value(repair)?, code))
        }
        Command::RepairStatus { id } => {
            let repair = maintenance.repairs.get(&id).await?;
            let code = if repair.is_some() { 0 } else { 1 };
            Ok((serde_json::to_value(repair)?, code))
        }
        Command::QuarantineOpen {
            seq,
            did,
            kind,
            repairs,
        } => {
            let quarantine = quarantine_seq(
                &maintenance.repair_context(&did),
                seq,
                &did,
                &kind,
                &repairs,
            )
            .await?;
            Ok((serde_json::to_value(quarantine)?, 0))
        }
        Command::QuarantineLocalReconciled { seq } => {
            maintenance.repairs.mark_local_reconciled(seq).await?;
            let quarantine = maintenance.repairs.quarantine(seq).await?;
            Ok((serde_json::to_value(quarantine)?, 0))
        }
        Command::QuarantineClose {
            seq,
            external,
            justification,
        } => {
            let did = match maintenance.repairs.quarantine(seq).await? {
                Some(quarantine) => quarantine.did,
                None => bail!("no quarantine for seq {seq}"),
            };
            let quarantine = close_quarantine(
                &maintenance.repair_context(&did),
                seq,
                external,
                justification.as_deref(),
            )
            .await?;
            Ok((serde_json::to_value(quarantine)?, 0))
        }
        Command::QuarantineStatus { seq } => {
            let quarantine = maintenance.repairs.quarantine(seq).await?;
            let code = if quarantine.is_some() { 0 } else { 1 };
            Ok((serde_json::to_value(quarantine)?, code))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Command>> {
        parse_args(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn parses_every_command() {
        assert_eq!(parse(&[]).unwrap(), None);
        assert_eq!(
            parse(&["--drain-did", "did:plc:a"]).unwrap().unwrap(),
            Command::Drain {
                did: "did:plc:a".to_owned(),
                timeout: Duration::from_secs(600)
            }
        );
        assert_eq!(
            parse(&["--drain-did", "did:plc:a", "--timeout-secs", "5"])
                .unwrap()
                .unwrap(),
            Command::Drain {
                did: "did:plc:a".to_owned(),
                timeout: Duration::from_secs(5)
            }
        );
        assert_eq!(
            parse(&["--repair-create", "r.json"]).unwrap().unwrap(),
            Command::RepairCreate {
                file: "r.json".into()
            }
        );
        assert_eq!(
            parse(&["--repair-run", "r1"]).unwrap().unwrap(),
            Command::RepairRun {
                id: "r1".to_owned(),
                timeout: Duration::from_secs(600)
            }
        );
        assert_eq!(
            parse(&["--repair-status", "r1"]).unwrap().unwrap(),
            Command::RepairStatus {
                id: "r1".to_owned()
            }
        );
        assert_eq!(
            parse(&[
                "--quarantine-open",
                "7",
                "--did",
                "did:plc:a",
                "--kind",
                "bad",
                "--repair",
                "r1",
                "--repair",
                "r2"
            ])
            .unwrap()
            .unwrap(),
            Command::QuarantineOpen {
                seq: 7,
                did: "did:plc:a".to_owned(),
                kind: "bad".to_owned(),
                repairs: vec!["r1".to_owned(), "r2".to_owned()]
            }
        );
        assert_eq!(
            parse(&["--quarantine-local-reconciled", "7"])
                .unwrap()
                .unwrap(),
            Command::QuarantineLocalReconciled { seq: 7 }
        );
        assert_eq!(
            parse(&[
                "--quarantine-close",
                "7",
                "--external",
                "accepted",
                "--justification",
                "checked"
            ])
            .unwrap()
            .unwrap(),
            Command::QuarantineClose {
                seq: 7,
                external: ExternalOutcome::Accepted,
                justification: Some("checked".to_owned())
            }
        );
        assert_eq!(
            parse(&["--quarantine-close", "7", "--external", "verified"])
                .unwrap()
                .unwrap(),
            Command::QuarantineClose {
                seq: 7,
                external: ExternalOutcome::Verified,
                justification: None
            }
        );
        assert_eq!(
            parse(&["--quarantine-status", "7"]).unwrap().unwrap(),
            Command::QuarantineStatus { seq: 7 }
        );
    }

    #[test]
    fn rejects_malformed_commands() {
        for args in [
            vec!["--drain-did"],
            vec!["--drain-did", "--timeout-secs"],
            vec!["--timeout-secs", "soon", "--drain-did", "did:plc:a"],
            vec!["--bogus"],
            vec!["--timeout-secs", "5"],
            vec!["--quarantine-open", "x", "--did", "d", "--kind", "k"],
            vec!["--quarantine-open", "7", "--kind", "k"],
            vec!["--quarantine-open", "7", "--did", "d"],
            vec!["--quarantine-close", "7"],
            vec!["--quarantine-close", "7", "--external", "maybe"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }
}
