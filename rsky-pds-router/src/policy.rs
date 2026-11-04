//! The routing policy and the write allowlist it mirrors, re-read from
//! disk within seconds of a change. A file that fails to parse leaves the
//! previous one in force.

use arc_swap::ArcSwap;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("unsupported policy version {0}")]
    Version(u32),
    #[error("{0}: {1}")]
    Read(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Parse(PathBuf, String),
    #[error("{0} is not a backend name")]
    Backend(String),
}

/// Which implementation answers by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Ts,
    Rsky,
}

impl Backend {
    fn parse(value: &str) -> Result<Self, PolicyError> {
        match value {
            "ts" => Ok(Self::Ts),
            "rsky" => Ok(Self::Rsky),
            other => Err(PolicyError::Backend(other.to_owned())),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ts => "ts",
            Self::Rsky => "rsky",
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawPolicy {
    version: u32,
    #[serde(default)]
    kill_switch: bool,
    #[serde(default)]
    reads: RawReads,
    #[serde(default)]
    writes: RawWrites,
}

#[derive(Debug, Default, Deserialize)]
struct RawReads {
    default: Option<String>,
    #[serde(default)]
    pin_rsky: Vec<String>,
    #[serde(default)]
    pin_ts: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawWrites {
    default: Option<String>,
    #[serde(default)]
    canary_rsky: Vec<String>,
    #[serde(default)]
    canary_fence: Vec<String>,
}

/// The routing policy in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub kill_switch: bool,
    pub reads_default: Backend,
    pub pin_rsky: HashSet<String>,
    pub pin_ts: HashSet<String>,
    pub writes_default: Backend,
    pub canary_rsky: HashSet<String>,
    pub canary_fence: HashSet<String>,
}

impl Default for Policy {
    /// Everything to the TypeScript PDS, nothing pinned: the safe state
    /// before any policy file is read.
    fn default() -> Self {
        Self {
            kill_switch: false,
            reads_default: Backend::Ts,
            pin_rsky: HashSet::new(),
            pin_ts: HashSet::new(),
            writes_default: Backend::Ts,
            canary_rsky: HashSet::new(),
            canary_fence: HashSet::new(),
        }
    }
}

impl Policy {
    pub fn parse(text: &str, origin: &Path) -> Result<Self, PolicyError> {
        let raw: RawPolicy = toml::from_str(text)
            .map_err(|e| PolicyError::Parse(origin.to_path_buf(), e.to_string()))?;
        if raw.version != 1 {
            return Err(PolicyError::Version(raw.version));
        }
        Ok(Self {
            kill_switch: raw.kill_switch,
            reads_default: Backend::parse(raw.reads.default.as_deref().unwrap_or("ts"))?,
            pin_rsky: raw.reads.pin_rsky.into_iter().collect(),
            pin_ts: raw.reads.pin_ts.into_iter().collect(),
            writes_default: Backend::parse(raw.writes.default.as_deref().unwrap_or("ts"))?,
            canary_rsky: raw.writes.canary_rsky.into_iter().collect(),
            canary_fence: raw.writes.canary_fence.into_iter().collect(),
        })
    }

    /// Whether any canary exists, which makes an unattributable mutation
    /// unsafe to forward.
    pub fn has_canaries(&self) -> bool {
        !self.canary_rsky.is_empty() || !self.canary_fence.is_empty()
    }

    /// Where reads for `did` go.
    pub fn read_backend(&self, did: Option<&str>) -> Backend {
        if self.kill_switch {
            return Backend::Ts;
        }
        match did {
            Some(did) if self.pin_rsky.contains(did) => Backend::Rsky,
            Some(did) if self.pin_ts.contains(did) => Backend::Ts,
            _ => self.reads_default,
        }
    }
}

/// The states an actor can have in the write allowlist the rsky process
/// enforces; the router mirrors them so a canary's writes are never sent
/// to a process that will refuse them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionState {
    Active,
    Draining,
    Maintenance(String),
    Absent,
}

#[derive(Debug, Deserialize)]
struct RawAllowlist {
    version: u32,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    entries: BTreeMap<String, RawEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawEntry {
    State(String),
    Maintenance { state: String, workflow_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    default: AdmissionState,
    entries: BTreeMap<String, AdmissionState>,
}

impl Default for Allowlist {
    /// Nothing admitted until the file is read.
    fn default() -> Self {
        Self {
            default: AdmissionState::Absent,
            entries: BTreeMap::new(),
        }
    }
}

impl Allowlist {
    pub fn parse(text: &str, origin: &Path) -> Result<Self, PolicyError> {
        let raw: RawAllowlist = toml::from_str(text)
            .map_err(|e| PolicyError::Parse(origin.to_path_buf(), e.to_string()))?;
        if raw.version != 1 {
            return Err(PolicyError::Version(raw.version));
        }
        let default = match raw.default.as_deref().unwrap_or("absent") {
            "active" => AdmissionState::Active,
            "absent" => AdmissionState::Absent,
            "draining" => AdmissionState::Draining,
            other => {
                return Err(PolicyError::Parse(
                    origin.to_path_buf(),
                    format!("default {other:?} is not active, absent, or draining"),
                ))
            }
        };
        let mut entries = BTreeMap::new();
        for (did, entry) in raw.entries {
            let state = match entry {
                RawEntry::State(state) => match state.as_str() {
                    "active" => AdmissionState::Active,
                    "draining" => AdmissionState::Draining,
                    "absent" => AdmissionState::Absent,
                    other => {
                        return Err(PolicyError::Parse(
                            origin.to_path_buf(),
                            format!("{did}: unknown state {other:?}"),
                        ))
                    }
                },
                RawEntry::Maintenance { state, workflow_id } if state == "maintenance" => {
                    AdmissionState::Maintenance(workflow_id)
                }
                RawEntry::Maintenance { state, .. } => {
                    return Err(PolicyError::Parse(
                        origin.to_path_buf(),
                        format!("{did}: unknown state {state:?}"),
                    ))
                }
            };
            entries.insert(did, state);
        }
        Ok(Self { default, entries })
    }

    pub fn state_of(&self, did: &str) -> AdmissionState {
        match self.entries.get(did) {
            Some(state) => state.clone(),
            None => self.default.clone(),
        }
    }
}

/// A file re-read when its modification time changes.
struct Watched<T> {
    path: PathBuf,
    current: ArcSwap<T>,
    seen: std::sync::Mutex<Option<SystemTime>>,
}

impl<T> Watched<T> {
    fn new(path: PathBuf, initial: T) -> Self {
        Self {
            path,
            current: ArcSwap::from_pointee(initial),
            seen: std::sync::Mutex::new(None),
        }
    }

    /// Re-reads the file when it changed; a file that fails to parse is
    /// reported and leaves the current value.
    fn reload(
        &self,
        parse: impl Fn(&str, &Path) -> Result<T, PolicyError>,
    ) -> Result<bool, PolicyError> {
        let modified = std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .map_err(|e| PolicyError::Read(self.path.clone(), e))?;
        {
            let seen = self.seen.lock().expect("watch state poisoned");
            if *seen == Some(modified) {
                return Ok(false);
            }
        }
        let text = std::fs::read_to_string(&self.path)
            .map_err(|e| PolicyError::Read(self.path.clone(), e))?;
        let value = parse(&text, &self.path)?;
        self.current.store(Arc::new(value));
        *self.seen.lock().expect("watch state poisoned") = Some(modified);
        Ok(true)
    }
}

/// The policy and allowlist the router routes by, kept current.
pub struct Routing {
    policy: Watched<Policy>,
    allowlist: Watched<Allowlist>,
}

impl Routing {
    pub fn new(policy_path: PathBuf, allowlist_path: PathBuf) -> Self {
        Self {
            policy: Watched::new(policy_path, Policy::default()),
            allowlist: Watched::new(allowlist_path, Allowlist::default()),
        }
    }

    pub fn policy(&self) -> Arc<Policy> {
        self.policy.current.load_full()
    }

    pub fn allowlist(&self) -> Arc<Allowlist> {
        self.allowlist.current.load_full()
    }

    /// Reads both files once; errors are returned so a start-up can refuse.
    pub fn load(&self) -> Result<(), PolicyError> {
        self.policy.reload(Policy::parse)?;
        self.allowlist.reload(Allowlist::parse)?;
        Ok(())
    }

    /// Reads both files when they changed, keeping the previous value on
    /// a failure.
    pub fn refresh(&self) {
        if let Err(err) = self.policy.reload(Policy::parse) {
            tracing::error!(%err, "policy not reloaded");
        }
        if let Err(err) = self.allowlist.reload(Allowlist::parse) {
            tracing::error!(%err, "allowlist not reloaded");
        }
    }

    /// Polls both files every `interval` for as long as the router runs.
    pub fn spawn_reloader(self: &Arc<Self>, interval: Duration) {
        let routing = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                routing.refresh();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = r#"
version = 1
kill_switch = false
[reads]
default = "ts"
pin_rsky = ["did:plc:rudy"]
pin_ts = ["did:plc:held"]
[writes]
default = "ts"
canary_rsky = ["did:plc:test"]
canary_fence = ["did:plc:fenced"]
"#;

    #[test]
    fn parses_the_policy_and_routes_reads_by_pin() {
        let policy = Policy::parse(POLICY, Path::new("policy.toml")).unwrap();
        assert!(!policy.kill_switch);
        assert!(policy.has_canaries());
        assert_eq!(policy.read_backend(Some("did:plc:rudy")), Backend::Rsky);
        assert_eq!(policy.read_backend(Some("did:plc:held")), Backend::Ts);
        assert_eq!(policy.read_backend(Some("did:plc:other")), Backend::Ts);
        assert_eq!(policy.read_backend(None), Backend::Ts);
        let mut phase_b = policy.clone();
        phase_b.reads_default = Backend::Rsky;
        assert_eq!(phase_b.read_backend(Some("did:plc:other")), Backend::Rsky);
        assert_eq!(phase_b.read_backend(Some("did:plc:held")), Backend::Ts);
        let mut killed = policy.clone();
        killed.kill_switch = true;
        assert_eq!(killed.read_backend(Some("did:plc:rudy")), Backend::Ts);
        assert_eq!(Backend::Rsky.as_str(), "rsky");
        assert_eq!(Policy::default().reads_default, Backend::Ts);
        assert!(!Policy::default().has_canaries());
    }

    #[test]
    fn rejects_bad_policies() {
        let path = Path::new("policy.toml");
        assert!(matches!(
            Policy::parse("version = 2", path),
            Err(PolicyError::Version(2))
        ));
        assert!(matches!(
            Policy::parse("version = 1\n[reads]\ndefault = \"nope\"", path),
            Err(PolicyError::Backend(_))
        ));
        assert!(matches!(
            Policy::parse("not toml at all [", path),
            Err(PolicyError::Parse(..))
        ));
        assert!(Policy::parse("version = 1", path).is_ok());
    }

    #[test]
    fn parses_the_allowlist_states() {
        let text = r#"
version = 1
default = "absent"
[entries]
"did:plc:a" = "active"
"did:plc:d" = "draining"
"did:plc:x" = "absent"
"did:plc:m" = { state = "maintenance", workflow_id = "wf-1" }
"#;
        let list = Allowlist::parse(text, Path::new("a.toml")).unwrap();
        assert_eq!(list.state_of("did:plc:a"), AdmissionState::Active);
        assert_eq!(list.state_of("did:plc:d"), AdmissionState::Draining);
        assert_eq!(list.state_of("did:plc:x"), AdmissionState::Absent);
        assert_eq!(
            list.state_of("did:plc:m"),
            AdmissionState::Maintenance("wf-1".into())
        );
        assert_eq!(list.state_of("did:plc:none"), AdmissionState::Absent);
        let open =
            Allowlist::parse("version = 1\ndefault = \"active\"", Path::new("a.toml")).unwrap();
        assert_eq!(open.state_of("did:plc:none"), AdmissionState::Active);
        let draining = Allowlist::parse(
            "version = 1\ndefault = \"draining\"\n[entries]\n\"did:plc:a\" = \"active\"",
            Path::new("a.toml"),
        )
        .unwrap();
        assert_eq!(draining.state_of("did:plc:none"), AdmissionState::Draining);
        assert_eq!(draining.state_of("did:plc:a"), AdmissionState::Active);
        assert_eq!(
            Allowlist::default().state_of("did:plc:none"),
            AdmissionState::Absent
        );
        for bad in [
            "version = 3",
            "version = 1\ndefault = \"maybe\"",
            "version = 1\n[entries]\n\"did:plc:a\" = \"asleep\"",
            "version = 1\n[entries]\n\"did:plc:a\" = { state = \"paused\", workflow_id = \"w\" }",
            "version = 1\n[entries]\n\"did:plc:a\" = 5",
        ] {
            assert!(Allowlist::parse(bad, Path::new("a.toml")).is_err(), "{bad}");
        }
    }

    #[test]
    fn files_are_reloaded_on_change_and_kept_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let policy_path = dir.path().join("policy.toml");
        let allowlist_path = dir.path().join("allowlist.toml");
        let routing = Routing::new(policy_path.clone(), allowlist_path.clone());
        assert!(matches!(routing.load(), Err(PolicyError::Read(..))));
        std::fs::write(&policy_path, POLICY).unwrap();
        std::fs::write(
            &allowlist_path,
            "version = 1\n[entries]\n\"did:plc:test\" = \"active\"",
        )
        .unwrap();
        routing.load().unwrap();
        assert_eq!(
            routing.policy().read_backend(Some("did:plc:rudy")),
            Backend::Rsky
        );
        assert_eq!(
            routing.allowlist().state_of("did:plc:test"),
            AdmissionState::Active
        );
        // unchanged files are not re-read
        assert!(!routing.policy.reload(Policy::parse).unwrap());
        // a broken rewrite keeps the previous policy
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&policy_path, "version = 9").unwrap();
        let later = SystemTime::now() + Duration::from_secs(5);
        std::fs::File::open(&policy_path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        routing.refresh();
        assert_eq!(
            routing.policy().read_backend(Some("did:plc:rudy")),
            Backend::Rsky
        );
        // a good rewrite takes effect
        std::fs::write(&policy_path, "version = 1\nkill_switch = true").unwrap();
        std::fs::File::open(&policy_path)
            .unwrap()
            .set_modified(later + Duration::from_secs(5))
            .unwrap();
        routing.refresh();
        assert!(routing.policy().kill_switch);
        let shared = Arc::new(routing);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            shared.spawn_reloader(Duration::from_millis(10));
            std::fs::write(&policy_path, "version = 1").unwrap();
            std::fs::File::open(&policy_path)
                .unwrap()
                .set_modified(later + Duration::from_secs(10))
                .unwrap();
            for _ in 0..50 {
                if !shared.policy().kill_switch {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(!shared.policy().kill_switch);
        });
    }
}
