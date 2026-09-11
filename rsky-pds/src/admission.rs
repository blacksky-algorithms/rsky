//! Which actors this process may write, and in what state.
//!
//! While another PDS implementation shares the data directory, every actor
//! has exactly one writer. The allowlist file names the actors this process
//! writes; an actor it does not name is refused at every write choke point,
//! and a named actor moves through `active`, `draining`, and `maintenance`
//! under an operator's control. The file is re-read when it changes; a file
//! that fails to parse is rejected and the previous allowlist stays in force.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

/// What this process may do for one actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionState {
    /// Mutations are admitted and every worker runs.
    Active,
    /// New mutations are refused; in-flight work completes and workers keep
    /// running until the actor's queues are empty.
    Draining,
    /// Public mutations are refused; only the workflow holding the actor's
    /// maintenance slot with this id may write, and workers keep running.
    Maintenance { workflow_id: String },
    /// Everything is refused, workers included.
    Absent,
}

impl AdmissionState {
    pub fn name(&self) -> &'static str {
        match self {
            AdmissionState::Active => "active",
            AdmissionState::Draining => "draining",
            AdmissionState::Maintenance { .. } => "maintenance",
            AdmissionState::Absent => "absent",
        }
    }

    pub fn workflow_id(&self) -> Option<&str> {
        match self {
            AdmissionState::Maintenance { workflow_id } => Some(workflow_id),
            _ => None,
        }
    }
}

/// A write was refused because the actor is not admitted for it.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("{did} is not admitted for writes on this server: {state}")]
pub struct NotAdmitted {
    pub did: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EntryFile {
    Named(String),
    Maintenance { state: String, workflow_id: String },
}

#[derive(Debug, Deserialize)]
struct AllowlistFile {
    version: u32,
    default: Option<String>,
    #[serde(default)]
    entries: HashMap<String, EntryFile>,
}

fn parse_named(name: &str) -> Result<AdmissionState> {
    match name {
        "active" => Ok(AdmissionState::Active),
        "draining" => Ok(AdmissionState::Draining),
        "absent" => Ok(AdmissionState::Absent),
        "maintenance" => bail!("a maintenance entry needs a workflow_id"),
        other => bail!("unknown admission state: {other}"),
    }
}

impl TryFrom<EntryFile> for AdmissionState {
    type Error = anyhow::Error;

    fn try_from(entry: EntryFile) -> Result<Self> {
        match entry {
            EntryFile::Named(name) => parse_named(&name),
            EntryFile::Maintenance { state, workflow_id } => {
                if state != "maintenance" {
                    bail!("only a maintenance entry carries a workflow_id, not {state}");
                }
                if workflow_id.is_empty() {
                    bail!("a maintenance entry needs a non-empty workflow_id");
                }
                Ok(AdmissionState::Maintenance { workflow_id })
            }
        }
    }
}

/// The parsed allowlist: a default for every actor the file does not name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    pub default: AdmissionState,
    pub entries: HashMap<String, AdmissionState>,
}

impl Allowlist {
    /// Every actor admitted: the behaviour of a server that owns its data
    /// directory alone.
    pub fn unrestricted() -> Self {
        Allowlist {
            default: AdmissionState::Active,
            entries: HashMap::new(),
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        let file: AllowlistFile = toml::from_str(text).context("allowlist is not valid TOML")?;
        if file.version != 1 {
            bail!("unsupported allowlist version: {}", file.version);
        }
        let default = match file.default {
            Some(name) => parse_named(&name)?,
            None => AdmissionState::Absent,
        };
        if matches!(default, AdmissionState::Draining) {
            bail!("the default state cannot be draining");
        }
        let mut entries = HashMap::new();
        for (did, entry) in file.entries {
            if !did.starts_with("did:") {
                bail!("allowlist entry is not a DID: {did}");
            }
            let state = AdmissionState::try_from(entry)
                .with_context(|| format!("allowlist entry for {did}"))?;
            entries.insert(did, state);
        }
        Ok(Allowlist { default, entries })
    }

    pub fn state_of(&self, did: &str) -> &AdmissionState {
        self.entries.get(did).unwrap_or(&self.default)
    }
}

/// The live allowlist, shared by every write choke point.
pub struct Admission {
    source: Option<PathBuf>,
    current: RwLock<Arc<Allowlist>>,
    loaded: Mutex<Option<SystemTime>>,
}

impl Admission {
    pub fn unrestricted() -> Self {
        Admission {
            source: None,
            current: RwLock::new(Arc::new(Allowlist::unrestricted())),
            loaded: Mutex::new(None),
        }
    }

    /// Loads the allowlist from `path`; the file must exist and parse.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let (allowlist, modified) = read_allowlist(&path)?;
        Ok(Admission {
            source: Some(path),
            current: RwLock::new(Arc::new(allowlist)),
            loaded: Mutex::new(Some(modified)),
        })
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub fn allowlist(&self) -> Arc<Allowlist> {
        self.current
            .read()
            .expect("allowlist lock poisoned")
            .clone()
    }

    /// Re-reads the file if it changed since the last load. A file that no
    /// longer parses is rejected and the current allowlist stays in force.
    /// Returns whether a new allowlist was installed.
    pub fn reload(&self) -> Result<bool> {
        let Some(path) = &self.source else {
            return Ok(false);
        };
        let modified = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .with_context(|| format!("allowlist {} is unreadable", path.display()))?;
        {
            let loaded = self.loaded.lock().expect("allowlist load time poisoned");
            if *loaded == Some(modified) {
                return Ok(false);
            }
        }
        let (allowlist, modified) = read_allowlist(path)?;
        *self.current.write().expect("allowlist lock poisoned") = Arc::new(allowlist);
        *self.loaded.lock().expect("allowlist load time poisoned") = Some(modified);
        tracing::info!(path = %path.display(), "write allowlist reloaded");
        Ok(true)
    }

    /// Re-reads the file every `interval` for the life of the process.
    pub fn spawn_reloader(self: &Arc<Self>, interval: Duration) {
        if self.source.is_none() {
            return;
        }
        let admission = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                if let Err(err) = admission.reload() {
                    tracing::error!(?err, "write allowlist was not reloaded");
                }
            }
        });
    }

    pub fn state_of(&self, did: &str) -> AdmissionState {
        self.allowlist().state_of(did).clone()
    }

    fn refuse(&self, did: &str, state: &AdmissionState) -> NotAdmitted {
        NotAdmitted {
            did: did.to_owned(),
            state: state.name().to_owned(),
        }
    }

    /// A client mutation: only an active actor accepts one.
    pub fn admit_mutation(&self, did: &str) -> Result<(), NotAdmitted> {
        let allowlist = self.allowlist();
        match allowlist.state_of(did) {
            AdmissionState::Active => Ok(()),
            state => Err(self.refuse(did, state)),
        }
    }

    /// Publication, blob, lifecycle, and repair work: runs for every actor
    /// this process names, in any state but absent.
    pub fn admit_worker(&self, did: &str) -> Result<(), NotAdmitted> {
        let allowlist = self.allowlist();
        match allowlist.state_of(did) {
            AdmissionState::Absent => Err(self.refuse(did, &AdmissionState::Absent)),
            _ => Ok(()),
        }
    }

    /// A repair or recovery transaction: only the workflow named by the
    /// actor's maintenance entry may write.
    pub fn admit_maintenance(&self, did: &str, workflow_id: &str) -> Result<(), NotAdmitted> {
        let allowlist = self.allowlist();
        match allowlist.state_of(did) {
            AdmissionState::Maintenance { workflow_id: held } if held == workflow_id => Ok(()),
            state => Err(self.refuse(did, state)),
        }
    }
}

fn read_allowlist(path: &Path) -> Result<(Allowlist, SystemTime)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("allowlist {} is unreadable", path.display()))?;
    let modified = std::fs::metadata(path)?.modified()?;
    let allowlist =
        Allowlist::parse(&text).with_context(|| format!("allowlist {}", path.display()))?;
    Ok((allowlist, modified))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"
version = 1
default = "absent"

[entries]
"did:plc:active" = "active"
"did:plc:draining" = "draining"
"did:plc:repair" = { state = "maintenance", workflow_id = "repair-7" }
"#;

    #[test]
    fn parses_every_state_and_the_default() {
        let list = Allowlist::parse(FILE).unwrap();
        assert_eq!(list.default, AdmissionState::Absent);
        assert_eq!(list.state_of("did:plc:active"), &AdmissionState::Active);
        assert_eq!(list.state_of("did:plc:draining"), &AdmissionState::Draining);
        assert_eq!(
            list.state_of("did:plc:repair"),
            &AdmissionState::Maintenance {
                workflow_id: "repair-7".to_owned()
            }
        );
        assert_eq!(list.state_of("did:plc:other"), &AdmissionState::Absent);
        assert_eq!(
            list.state_of("did:plc:repair").workflow_id(),
            Some("repair-7")
        );
        assert_eq!(AdmissionState::Draining.name(), "draining");
        assert_eq!(AdmissionState::Active.workflow_id(), None);

        let open = Allowlist::parse("version = 1\ndefault = \"active\"\n").unwrap();
        assert_eq!(open.state_of("did:plc:anyone"), &AdmissionState::Active);
        let implicit = Allowlist::parse("version = 1\n").unwrap();
        assert_eq!(implicit.default, AdmissionState::Absent);
        assert_eq!(Allowlist::unrestricted().default, AdmissionState::Active);
    }

    #[test]
    fn rejects_malformed_files() {
        for (text, needle) in [
            ("not toml [", "valid TOML"),
            ("version = 2\n", "version"),
            ("version = 1\ndefault = \"draining\"\n", "default"),
            ("version = 1\ndefault = \"whatever\"\n", "unknown admission state"),
            (
                "version = 1\n[entries]\n\"did:plc:a\" = \"maintenance\"\n",
                "workflow_id",
            ),
            (
                "version = 1\n[entries]\n\"did:plc:a\" = { state = \"active\", workflow_id = \"x\" }\n",
                "only a maintenance entry",
            ),
            (
                "version = 1\n[entries]\n\"did:plc:a\" = { state = \"maintenance\", workflow_id = \"\" }\n",
                "non-empty",
            ),
            ("version = 1\n[entries]\nalice = \"active\"\n", "not a DID"),
        ] {
            let err = Allowlist::parse(text).unwrap_err();
            assert!(
                format!("{err:#}").contains(needle),
                "{text}: {err:#} lacks {needle}"
            );
        }
    }

    #[test]
    fn admission_decisions_follow_the_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("write-allowlist.toml");
        std::fs::write(&path, FILE).unwrap();
        let admission = Admission::from_file(&path).unwrap();
        assert_eq!(admission.source(), Some(path.as_path()));

        assert!(admission.admit_mutation("did:plc:active").is_ok());
        assert!(admission.admit_worker("did:plc:active").is_ok());
        assert_eq!(
            admission.admit_mutation("did:plc:draining"),
            Err(NotAdmitted {
                did: "did:plc:draining".to_owned(),
                state: "draining".to_owned()
            })
        );
        assert!(admission.admit_worker("did:plc:draining").is_ok());
        assert!(admission.admit_mutation("did:plc:repair").is_err());
        assert!(admission.admit_worker("did:plc:repair").is_ok());
        assert!(admission
            .admit_maintenance("did:plc:repair", "repair-7")
            .is_ok());
        assert!(admission
            .admit_maintenance("did:plc:repair", "repair-8")
            .is_err());
        assert!(admission
            .admit_maintenance("did:plc:active", "repair-7")
            .is_err());
        let absent = admission.admit_worker("did:plc:other").unwrap_err();
        assert_eq!(absent.state, "absent");
        assert!(absent.to_string().contains("not admitted"));
        assert_eq!(admission.state_of("did:plc:other"), AdmissionState::Absent);

        let open = Admission::unrestricted();
        assert!(open.admit_mutation("did:plc:anyone").is_ok());
        assert!(!open.reload().unwrap());
        assert!(open.source().is_none());
    }

    #[test]
    fn reload_installs_changes_and_keeps_the_last_good_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("write-allowlist.toml");
        std::fs::write(&path, FILE).unwrap();
        let admission = Admission::from_file(&path).unwrap();
        assert!(
            !admission.reload().unwrap(),
            "unchanged file is not reloaded"
        );

        // a change to the file is picked up
        std::thread::sleep(Duration::from_millis(20));
        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        std::fs::write(&path, "version = 1\ndefault = \"active\"\n").unwrap();
        std::fs::File::open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(admission.reload().unwrap());
        assert!(admission.admit_mutation("did:plc:other").is_ok());

        // a broken file is rejected and the previous allowlist stays
        std::fs::write(&path, "version = 3\n").unwrap();
        std::fs::File::open(&path)
            .unwrap()
            .set_modified(later + Duration::from_secs(5))
            .unwrap();
        assert!(admission.reload().is_err());
        assert!(admission.admit_mutation("did:plc:other").is_ok());

        // a missing file cannot be loaded at all
        assert!(Admission::from_file(dir.path().join("missing.toml")).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(admission.reload().is_err());
    }

    #[tokio::test]
    async fn reloader_polls_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("write-allowlist.toml");
        std::fs::write(&path, FILE).unwrap();
        let admission = Arc::new(Admission::from_file(&path).unwrap());
        admission.spawn_reloader(Duration::from_millis(10));
        Arc::new(Admission::unrestricted()).spawn_reloader(Duration::from_millis(10));
        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        std::fs::write(&path, "version = 1\ndefault = \"active\"\n").unwrap();
        std::fs::File::open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        for _ in 0..100 {
            if admission.admit_mutation("did:plc:other").is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(admission.admit_mutation("did:plc:other").is_ok());
        // a broken file is logged and the previous allowlist stays
        std::fs::write(&path, "version = 3\n").unwrap();
        std::fs::File::open(&path)
            .unwrap()
            .set_modified(later + Duration::from_secs(5))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(admission.admit_mutation("did:plc:other").is_ok());
    }
}
