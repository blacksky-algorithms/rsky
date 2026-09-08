//! per-repo state in storage.
//!
//! see `repo` for public-facing repo view of combined state.
//!
//! "ri|"||<DID> => <DbRepoInfo:cbor>
//!
//! todo: rate-limit resyncs per-did (persisted?, maybe?)
//! todo: per-upstream account status, and moderated account status
//! todo: do we have last-desync-reason kept around?

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use dasl::drisl;
use serde::{Deserialize, Serialize};

use super::{DecodeError, LoadError, PREFIX_REPO_INFO, unix_ms_u64};
use crate::host::{Host, HostnameError};
use crate::{
    Did, DidMethod, HostRegistry, SigningKey, StorageBatch, StorageEngine, StorageError, Tid,
};

const MAX_BACKOFF: Duration = Duration::from_secs(86_400);

/// raw repo stuff -- see [`RepoInfo`] for the nice type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DbRepoInfo {
    upstream_status: AccountStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    moderation: Option<Moderation>,
    sync_status: SyncStatus,
    identity: DbRepoIdentity,
    #[serde(with = "unix_ms_u64")]
    first_seen_at: SystemTime,
    resyncs: u32,
    pds_changes: u32,
    handle_changes: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_resync: Option<DbResyncInfo>,
    // hint for when initial resync fails and a permit was needed.
    // always cleared after initial resync succeeds.
    #[serde(default, skip_serializing_if = "is_false")]
    first_resync_needs_permit: bool,
}
fn is_false(b: &bool) -> bool {
    !*b
}

impl DbRepoInfo {
    pub(super) fn key(did: &Did) -> Vec<u8> {
        let did = did.as_str().as_bytes();
        let mut k = Vec::with_capacity(PREFIX_REPO_INFO.len() + did.len());
        k.extend_from_slice(PREFIX_REPO_INFO);
        k.extend_from_slice(did);
        k
    }
}

/// nice repo stuff (interned host)
#[derive(Debug, Clone)]
pub struct RepoInfo {
    /// The *upstream* account status
    ///
    /// Usually the account status should be read through `.account_status()`,
    /// which takes local moderation state into account.
    pub upstream_status: AccountStatus,
    pub moderation: Option<Moderation>,
    pub sync_status: SyncStatus,
    pub identity: RepoIdentity,
    pub first_seen_at: SystemTime,
    pub resyncs: u32,
    pub pds_changes: u32,
    pub handle_changes: u32,
    pub last_resync: Option<ResyncInfo>,
    /// initial resync retry hint, exclusive with presence of `last_resync`
    ///
    /// TODO: we could use an enum for the in-memory RepoInfo repr here instead
    /// of needing to manually enforce exclusion with `last_resync`.
    pub first_resync_needs_permit: Option<()>,
}

impl RepoInfo {
    /// get the effective account status, from upstream + local moderation
    pub fn account_status(&self) -> AccountStatus {
        match (&self.upstream_status, self.moderation) {
            (AccountStatus::Deleted, _local) => AccountStatus::Deleted,
            (_up, Some(moderated)) => moderated.action.into(),
            (upstream, None) => upstream.clone(),
        }
    }

    /// shortcut for the whether we're *effectively* active
    pub fn is_active(&self) -> bool {
        self.moderation.is_none() && matches!(self.upstream_status, AccountStatus::Active)
    }

    pub fn load<S: StorageEngine>(
        storage: &S,
        registry: &HostRegistry,
        did: &Did,
    ) -> Result<Option<Self>, LoadError<S::Error>> {
        let Some(bytes) = storage
            .get(&DbRepoInfo::key(did))
            .map_err(LoadError::Storage)?
        else {
            return Ok(None);
        };
        let db_info: DbRepoInfo = drisl::from_slice(&bytes)?;
        let info = Self::from_db_repo_info(registry, db_info)?;
        Ok(Some(info))
    }

    pub fn exists<S: StorageEngine>(storage: &S, did: &Did) -> Result<bool, LoadError<S::Error>> {
        // TODO: storage engine .exists -> bool by key
        Ok(storage
            .get(&DbRepoInfo::key(did))
            .map_err(LoadError::Storage)?
            .is_some())
    }

    /// scan the whole repo-info keyspace, yielding the DID of every repo
    /// currently parked [`SyncStatus::OutOfScope`].
    ///
    /// only the raw db shape is decoded (no host re-interning).
    /// non-out-of-scope repos are skipped.
    pub(crate) fn scan_out_of_scope<S: StorageEngine>(
        storage: &S,
    ) -> impl Iterator<Item = Result<Did, LoadError<S::Error>>> + '_ {
        storage
            .scan_from(PREFIX_REPO_INFO, b"")
            .filter_map(|pair| match pair {
                Err(e) => Some(Err(LoadError::Storage(e))),
                Ok((key, value)) => {
                    // skip slots
                    if key.contains(&0x00) {
                        return None;
                    }
                    let db: DbRepoInfo = match drisl::from_slice(&value) {
                        Ok(db) => db,
                        Err(e) => return Some(Err(e.into())),
                    };
                    if !matches!(db.sync_status, SyncStatus::OutOfScope { .. }) {
                        return None;
                    }
                    Some(did_from_info_key(&key).map_err(LoadError::Decode))
                }
            })
    }

    pub fn store<E, B>(&self, did: &Did, batch: &mut B)
    where
        E: StorageError,
        B: StorageBatch<E>,
    {
        let db_info: DbRepoInfo = self.clone().into();
        let bytes = drisl::to_vec(&db_info).expect("drisl to_vec infallible?");
        batch.put(&DbRepoInfo::key(did), &bytes);
    }

    fn from_db_repo_info(
        registry: &HostRegistry,
        db_info: DbRepoInfo,
    ) -> Result<Self, HostnameError> {
        let id = db_info.identity;
        Ok(Self {
            sync_status: db_info.sync_status,
            moderation: db_info.moderation,
            upstream_status: db_info.upstream_status,
            identity: RepoIdentity {
                pds_host: registry.get(&id.pds)?,
                signing_key: id.signing_key,
                supposed_handle: id.supposed_handle,
                resolved_at: id.resolved_at,
            },
            first_seen_at: db_info.first_seen_at,
            resyncs: db_info.resyncs,
            pds_changes: db_info.pds_changes,
            handle_changes: db_info.handle_changes,
            last_resync: db_info
                .last_resync
                .map(|r| ResyncInfo::from_db(registry, r))
                .transpose()?,
            first_resync_needs_permit: db_info.first_resync_needs_permit.then_some(()),
        })
    }
}

/// reconstruct a DID from a repo-info key (prefix-stripped)
fn did_from_info_key(key: &[u8]) -> Result<Did, DecodeError> {
    std::str::from_utf8(key)
        .map_err(|err| DecodeError::NotUtf8 { what: "did", err })?
        .try_into()
        .map_err(DecodeError::BadDid)
}

impl From<RepoInfo> for DbRepoInfo {
    fn from(info: RepoInfo) -> Self {
        let id = info.identity;
        Self {
            sync_status: info.sync_status,
            moderation: info.moderation,
            upstream_status: info.upstream_status,
            identity: DbRepoIdentity {
                pds: id.pds_host.name().as_str().to_string(),
                signing_key: id.signing_key,
                supposed_handle: id.supposed_handle,
                resolved_at: id.resolved_at,
            },
            first_seen_at: info.first_seen_at,
            resyncs: info.resyncs,
            pds_changes: info.pds_changes,
            handle_changes: info.handle_changes,
            last_resync: info.last_resync.map(Into::into),
            first_resync_needs_permit: info.first_resync_needs_permit.is_some(),
        }
    }
}

/// raw repo identity -- see [`RepoIdentity`] for the nice version
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbRepoIdentity {
    pds: String,
    signing_key: SigningKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    supposed_handle: Option<String>,
    #[serde(with = "unix_ms_u64")]
    resolved_at: SystemTime,
}

/// nice repo identity stuff (interned host)
#[derive(Clone)]
pub struct RepoIdentity {
    pub pds_host: Arc<Host>,
    pub signing_key: SigningKey,
    pub supposed_handle: Option<String>,
    pub resolved_at: SystemTime,
}
impl fmt::Debug for RepoIdentity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RepoIdentity")
            .field(
                "supposed_handle",
                &self.supposed_handle.as_deref().unwrap_or("[missing]"),
            )
            .field("pds_host", &self.pds_host.name().as_str())
            .field("signing_key", &self.signing_key)
            .finish_non_exhaustive()
    }
}

impl RepoIdentity {
    pub fn age(&self, now: SystemTime) -> Duration {
        now.duration_since(self.resolved_at)
            .unwrap_or(Duration::ZERO)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountStatus {
    Active,
    Deleted,
    Deactivated,
    Suspended,
    Takendown,
    /// fallback for unrecognized non-active status
    Inactive(String),
}

impl AccountStatus {
    pub fn from_list_status(status: Option<&str>) -> Self {
        match status {
            Some("takendown") => AccountStatus::Takendown,
            Some("suspended") => AccountStatus::Suspended,
            Some("deleted") => AccountStatus::Deleted,
            Some("deactivated") => AccountStatus::Deactivated,
            Some(other) => AccountStatus::Inactive(other.to_string()),
            None => AccountStatus::Inactive("no_status".to_string()),
        }
    }
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
    pub fn is_gone(&self) -> bool {
        matches!(self, Self::Deleted | Self::Takendown)
    }
    pub fn kind(&self) -> AccountStatusKind {
        match self {
            Self::Active => AccountStatusKind::Active,
            Self::Deactivated => AccountStatusKind::Deactivated,
            Self::Suspended => AccountStatusKind::Suspended,
            Self::Takendown => AccountStatusKind::Takendown,
            Self::Deleted => AccountStatusKind::Deleted,
            Self::Inactive(_) => AccountStatusKind::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatusKind {
    Active,
    Deactivated,
    Suspended,
    Takendown,
    Deleted,
    Other,
}

impl AccountStatusKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deactivated => "deactivated",
            Self::Suspended => "suspended",
            Self::Takendown => "takendown",
            Self::Deleted => "deleted",
            Self::Other => "other",
        }
    }
}

/// where we got an account status from
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountStatusSource {
    /// authoritative for any repo (but we still record which relay)
    Relay(String),
    /// a specific pds, only authoritative for repos that point to it
    ///
    /// must be hostname-normalized
    Pds(String),
}

impl AccountStatusSource {
    pub fn is_authoritative_for(&self, identity: &RepoIdentity) -> bool {
        match self {
            AccountStatusSource::Relay(_) => true,
            AccountStatusSource::Pds(host) => identity.pds_host.name().as_str() == host.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Moderation {
    pub action: ModAction,
    #[serde(with = "unix_ms_u64")]
    pub at: SystemTime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ModAction {
    #[serde(rename = "suspend")]
    Suspend,
    #[serde(rename = "takedown")]
    Takedown,
}

impl From<ModAction> for AccountStatus {
    fn from(m: ModAction) -> Self {
        match m {
            ModAction::Suspend => AccountStatus::Suspended,
            ModAction::Takedown => AccountStatus::Takendown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncStatus {
    Synchronized,
    Desynchronized(Desynchronized),
    OutOfScope {
        #[serde(with = "unix_ms_u64")]
        since: SystemTime,
    },
    /// non-active account status made our sync state (if any) become (or be
    /// initialized as) stale.
    ///
    /// atproto sync does *not* guarantee that we'll see a reactivation #account
    /// event over the firehose, so there is a risk of accounts getting stuck
    /// here. two mitigations:
    ///
    /// 1. if we see a firehose event for a NonActive repo, we should probably
    ///    check upstream to see if they've lifted a non-active status
    ///
    /// 2. a periodic upstream re-crawl (listRepos etc) can get us eventual
    ///    consistency of account status
    NonActive {
        #[serde(with = "unix_ms_u64")]
        since: SystemTime,
        #[serde(with = "crate::tid::tid_raw_u64_opt")]
        rev: Option<Tid>,
    },
    /// account deleted or otherwise permanently gone
    Gone {
        #[serde(with = "unix_ms_u64")]
        since: SystemTime,
        reason: String,
    },
}

impl SyncStatus {
    pub fn kind(&self) -> SyncStatusKind {
        match self {
            Self::Synchronized => SyncStatusKind::Synchronized,
            Self::Desynchronized(_) => SyncStatusKind::Desynchronized,
            Self::OutOfScope { .. } => SyncStatusKind::OutOfScope,
            Self::NonActive { .. } => SyncStatusKind::NonActive,
            Self::Gone { .. } => SyncStatusKind::Gone,
        }
    }

    pub fn name(&self) -> &'static str {
        self.kind().name()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatusKind {
    Synchronized,
    Desynchronized,
    OutOfScope,
    NonActive,
    Gone,
}

impl SyncStatusKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Synchronized => "synchronized",
            Self::Desynchronized => "desynchronized",
            Self::OutOfScope => "outOfScope",
            Self::NonActive => "nonActive",
            Self::Gone => "gone",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Desynchronized {
    pub reason: DesyncReason,
    #[serde(with = "unix_ms_u64")]
    due_at: SystemTime,
    pub resync_attempts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_resync_error: Option<String>,
}

impl Desynchronized {
    pub fn new(
        reason: DesyncReason,
        due: SystemTime,
        resync_attempts: u32,
        last_resync_error: Option<&'static str>,
    ) -> Self {
        Self {
            reason,
            due_at: crate::storage::unix_ms_u64::floor(due),
            resync_attempts,
            last_resync_error: last_resync_error.map(Into::into),
        }
    }
    /// check if this desync time matches another
    ///
    /// normalizes the time to storage resolution for comparison
    pub fn is_at(&self, other: SystemTime) -> bool {
        let other = crate::storage::unix_ms_u64::floor(other);
        other == self.due_at
    }

    pub fn due_at(&self) -> SystemTime {
        self.due_at
    }

    pub fn set_due(&mut self, t: SystemTime) {
        self.due_at = crate::storage::unix_ms_u64::floor(t);
    }

    fn backoff_from(bottom: u64, base: f64, attempts: u32) -> u64 {
        bottom * base.powf(attempts as f64).min(u64::MAX as f64) as u64
    }

    fn backoff_from_zero(base: f64, attempts: u32) -> u64 {
        if attempts == 0 {
            return 0;
        }
        base.powf(attempts as f64).min(u64::MAX as f64) as u64
    }

    pub fn backoff(reason: &DesyncReason, attempts: u32, method: DidMethod) -> Option<Duration> {
        match reason {
            #[allow(
                clippy::match_overlapping_arm,
                reason = "visual alignment is actually clearer here i think"
            )]
            DesyncReason::UnresolvableIdentity => match method {
                DidMethod::Plc => match attempts {
                    ..=1 => Some(60),
                    ..=2 => Some(300),
                    _ => Some(3600), // todo *definitely* need to handle not-found
                },
                DidMethod::Web => match attempts {
                    ..=1 => Some(60),
                    ..=4 => Some(300),
                    ..=8 => Some(3600),
                    ..=48 => Some(MAX_BACKOFF.as_secs()),
                    _ => None, // eventuallyyyyyyyyyyyyy
                },
            },
            DesyncReason::FirstSeen => Some(Self::backoff_from_zero(1.5, attempts)),
            DesyncReason::CameIntoScope => Some(Self::backoff_from_zero(1.5, attempts)),
            DesyncReason::FutureRev { rev } => {
                let now = SystemTime::now();
                let (t, _) = (*rev).into();
                Some(match t.duration_since(now) {
                    Ok(time_left) => time_left.as_secs() + 1,
                    Err(_) => Self::backoff_from(30, 1.5, attempts),
                })
            }
            DesyncReason::FirehoseSync { .. } => Some(Self::backoff_from_zero(2., attempts)),
            DesyncReason::FirehoseAccountDesynchronized => {
                Some(Self::backoff_from_zero(2., attempts))
            }
            DesyncReason::FirehoseAccountThrottled => {
                // ehhhhh idk if this is right!
                // cool them off for at least 5mins then resync.
                Some(300.max(Self::backoff_from_zero(2., attempts)))
            }
            DesyncReason::Sync11Lax => {
                // up to twice a day lax catch-ups
                Some(12 * 3600)
            }
            DesyncReason::FirehoseFail { .. } => Some(Self::backoff_from_zero(2., attempts)),
            DesyncReason::Throttled { .. } => Some(Self::backoff_from(600, 1.1, attempts)),
            DesyncReason::AppRequested { .. } => Some(Self::backoff_from_zero(1.5, attempts)),
        }
        .map(|s| Duration::from_secs(s).min(MAX_BACKOFF))
    }

    /// test whether the reason is the same as ours
    ///
    /// note that as of this moment, hubble-sync itself never changes reason if
    /// eg., resync fails.
    pub fn is_same_reason(&self, o: &DesyncReason) -> bool {
        use DesyncReason as R;
        match self.reason {
            R::FirstSeen => matches!(o, R::FirstSeen),
            R::CameIntoScope => matches!(o, R::CameIntoScope),
            R::UnresolvableIdentity => matches!(o, R::UnresolvableIdentity),
            R::FirehoseSync { .. } => matches!(o, R::FirehoseSync { .. }),
            R::FirehoseFail { .. } => matches!(o, R::FirehoseFail { .. }),
            R::FutureRev { .. } => matches!(o, R::FutureRev { .. }),
            R::Throttled { .. } => matches!(o, R::Throttled { .. }),
            R::Sync11Lax => matches!(o, R::Sync11Lax),
            R::AppRequested { .. } => matches!(o, R::AppRequested { .. }),
            R::FirehoseAccountDesynchronized => matches!(o, R::FirehoseAccountDesynchronized),
            R::FirehoseAccountThrottled => matches!(o, R::FirehoseAccountThrottled),
        }
    }

    pub fn error_changed(&self, o: Option<&str>) -> bool {
        if let Some(ours) = &self.last_resync_error
            && let Some(yours) = o
        {
            return yours != ours;
        }
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DesyncReason {
    /// from the firehose, or backfill discovery, or manually added
    FirstSeen,
    /// a previously-out-of-scope repo became in-scope
    CameIntoScope,
    /// could not resolve a repo from discovery or in some other scenarios
    ///
    /// we can't accept any data if we can't get the signing key to verify it
    UnresolvableIdentity,
    /// a #sync event on the firehose
    ///
    /// #sync events are ignored when `rev` and `prev_data` fields don't change
    ///
    /// #sync events with a change also don't *necessarily* lead to
    /// desynchronized state -- the actor is allowed to attempt a resync inline.
    ///
    /// but, if it doesn't, or if that fails, this was the source of that.
    FirehoseSync {
        #[serde(with = "crate::tid::tid_raw_u64")]
        rev: Tid,
    },
    /// sync1.1 proof validation failure on a #commit
    ///
    /// commits dropped otherwise (signature fail, old `rev`,) don't resync.
    FirehoseFail {
        #[serde(with = "crate::tid::tid_raw_u64_opt")]
        bad_commit_rev: Option<Tid>,
    },
    /// upstream #account event indicated that the repo is out of sync
    FirehoseAccountDesynchronized,
    /// upstream #account event indicated that the repo is throttled
    FirehoseAccountThrottled,
    /// commit rev too far in the future
    FutureRev {
        #[serde(with = "crate::tid::tid_raw_u64")]
        rev: Tid,
    },
    /// repo-level or host-level firehose rate-limits were applied
    Throttled { exceeded: Option<String> },
    /// pds is not emitting sync1.1
    Sync11Lax,
    /// the consumer application requested that this repo be desynchronized
    AppRequested {
        reason: String,
        #[serde(with = "crate::tid::tid_raw_u64_opt")]
        at_commit_rev: Option<Tid>,
    },
}

impl DesyncReason {
    pub fn kind(&self) -> DesyncReasonKind {
        match self {
            Self::FirstSeen => DesyncReasonKind::FirstSeen,
            Self::CameIntoScope => DesyncReasonKind::CameIntoScope,
            Self::UnresolvableIdentity => DesyncReasonKind::UnresolvableIdentity,
            Self::FirehoseSync { .. } => DesyncReasonKind::FirehoseSync,
            Self::FirehoseFail { .. } => DesyncReasonKind::FirehoseFail,
            Self::FirehoseAccountDesynchronized => DesyncReasonKind::FirehoseAccountDesynchronized,
            Self::FirehoseAccountThrottled => DesyncReasonKind::FirehoseAccountThrottled,
            Self::FutureRev { .. } => DesyncReasonKind::FutureRev,
            Self::Throttled { .. } => DesyncReasonKind::Throttled,
            Self::Sync11Lax => DesyncReasonKind::Sync11Lax,
            Self::AppRequested { .. } => DesyncReasonKind::AppRequested,
        }
    }

    pub fn name(&self) -> &'static str {
        self.kind().name()
    }
}

/// fieldless mirror of [`DesyncReason`], for count buckets + metric labels
///
/// (mirrors [`SyncStatusKind`]/[`AccountStatusKind`])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesyncReasonKind {
    FirstSeen,
    CameIntoScope,
    UnresolvableIdentity,
    FirehoseSync,
    FirehoseFail,
    FirehoseAccountDesynchronized,
    FirehoseAccountThrottled,
    FutureRev,
    Throttled,
    Sync11Lax,
    AppRequested,
}

impl DesyncReasonKind {
    /// every variant, for iterating count buckets / emitting gauges
    pub const ALL: &'static [DesyncReasonKind] = &[
        Self::FirstSeen,
        Self::CameIntoScope,
        Self::UnresolvableIdentity,
        Self::FirehoseSync,
        Self::FirehoseFail,
        Self::FirehoseAccountDesynchronized,
        Self::FirehoseAccountThrottled,
        Self::FutureRev,
        Self::Throttled,
        Self::Sync11Lax,
        Self::AppRequested,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::FirstSeen => "first_seen",
            Self::CameIntoScope => "came_into_scope",
            Self::UnresolvableIdentity => "unresolvable_identity",
            Self::FirehoseSync => "firehose_sync",
            Self::FirehoseFail => "firehose_fail",
            Self::FirehoseAccountDesynchronized => "firehose_account_desync",
            Self::FirehoseAccountThrottled => "firehose_account_throttled",
            Self::FutureRev => "future_rev",
            Self::Throttled => "throttled",
            Self::Sync11Lax => "sync11_lax",
            Self::AppRequested => "app_requested",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResyncInfo {
    pub at: SystemTime,
    /// resync data size (meaning up to ResyncData impl)
    ///
    /// for the default impl: car block bytes
    pub size: Option<i64>,
    /// resync data count (meaning up to ResyncData impl)
    ///
    /// for the default impl: the number of records emited during apply_resync
    pub count: Option<i32>,
    /// resync data source
    ///
    /// usually a pds, but could be a fallback, etc. (manual upload??)
    ///
    /// for the default data source, this is the final host after resolving
    /// redirects.
    ///
    /// if `resync()` does not set `resolved` on `ResyncContext`, the upstream
    /// host will be used.
    from_host: Option<Arc<Host>>,
    /// how long the resync took, if available
    ///
    /// use an accessor for the nicely-typed value
    duration_ms: Option<u32>,
}

impl ResyncInfo {
    pub fn new(
        at: SystemTime,
        size: Option<i64>,
        count: Option<i32>,
        from_host: Option<Arc<Host>>,
        duration: Option<Duration>,
    ) -> Self {
        Self {
            at,
            size,
            count,
            from_host,
            duration_ms: duration.map(|d| d.as_millis() as u32),
        }
    }
    /// the (interned) host the resync data came from, if known
    pub fn host(&self) -> Option<&Arc<Host>> {
        self.from_host.as_ref()
    }
    pub fn duration(&self) -> Option<Duration> {
        let ms = self.duration_ms?;
        Some(Duration::from_millis(ms.into()))
    }

    fn from_db(registry: &HostRegistry, db: DbResyncInfo) -> Result<Self, HostnameError> {
        Ok(Self {
            at: db.at,
            size: db.size,
            count: db.count,
            from_host: db.from_host.map(|h| registry.get(&h)).transpose()?,
            duration_ms: db.duration_ms,
        })
    }
}

/// raw resync stats -- see [`ResyncInfo`] for the nice version
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbResyncInfo {
    #[serde(with = "unix_ms_u64")]
    at: SystemTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u32>,
}

impl From<ResyncInfo> for DbResyncInfo {
    fn from(info: ResyncInfo) -> Self {
        Self {
            at: info.at,
            size: info.size,
            count: info.count,
            from_host: info.from_host.map(|h| h.name().as_str().to_string()),
            duration_ms: info.duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::super::DecodeError;
    use super::*;
    use crate::StorageBatch;
    use crate::host::Hostname;
    use crate::identity::SigningKey;
    use crate::storage::engine::mem::MemEngine;

    fn h(s: &str) -> Arc<Host> {
        Arc::new(Host::raw(Hostname::new(s)))
    }
    fn t(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }
    fn registry() -> Arc<HostRegistry> {
        HostRegistry::new_default()
    }
    fn sample_info(host: Arc<Host>) -> RepoInfo {
        RepoInfo {
            upstream_status: AccountStatus::Active,
            moderation: None,
            sync_status: SyncStatus::Synchronized,
            identity: RepoIdentity {
                pds_host: host,
                signing_key: SigningKey::raw(vec![1, 2, 3, 4]),
                supposed_handle: Some("alice.example".into()),
                resolved_at: t(1_000),
            },
            first_seen_at: t(1_000),
            resyncs: 0,
            pds_changes: 0,
            handle_changes: 0,
            last_resync: None,
            first_resync_needs_permit: None,
        }
    }

    #[test]
    fn repo_info_load_returns_none_when_unstored() {
        let eng = MemEngine::new();
        let reg = registry();
        let got = RepoInfo::load(&eng, &reg, &Did::raw("did:plc:abc")).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn exists_reflects_repo_presence() {
        // the cheap probe the pending scheduler reconciles against: false while
        // absent, true once a repo info is stored (no parse/host-intern needed).
        let eng = MemEngine::new();
        let did = Did::raw("did:plc:abc");
        assert!(!RepoInfo::exists(&eng, &did).unwrap(), "absent");

        let mut b = eng.batch();
        sample_info(h("example.com")).store(&did, &mut b);
        b.commit().unwrap();

        assert!(RepoInfo::exists(&eng, &did).unwrap(), "present after store");
    }

    #[test]
    fn scan_out_of_scope_yields_only_parked_repos() {
        // the sweep's enumerator: yields exactly the OutOfScope DIDs, skipping
        // synchronized and gone repos, decoding only the raw db shape. DIDs are
        // re-validated out of the key, so they must be well-formed (32 chars).
        let eng = MemEngine::new();
        let host = h("example.com");
        let plc = |tag: char| Did::new(format!("did:plc:{}", tag.to_string().repeat(24))).unwrap();
        let store = |did: &Did, status: SyncStatus| {
            let mut info = sample_info(host.clone());
            info.sync_status = status;
            let mut b = eng.batch();
            info.store(did, &mut b);
            b.commit().unwrap();
        };

        store(&plc('a'), SyncStatus::OutOfScope { since: t(1) });
        store(&plc('b'), SyncStatus::Synchronized);
        store(&plc('c'), SyncStatus::OutOfScope { since: t(2) });
        store(
            &plc('d'),
            SyncStatus::Gone {
                since: t(3),
                reason: "deleted".into(),
            },
        );

        // an app slot colocated at `<info-key>\x00u` (see info_slot): its bytes
        // are not a DbRepoInfo (here, a bare CBOR uint) and the scan must skip
        // it rather than try to decode it as repo info.
        let slot_key = {
            let mut k = DbRepoInfo::key(&plc('a'));
            k.extend_from_slice(&[0x00, b'u']);
            k
        };
        let mut b = eng.batch();
        b.put(&slot_key, &[0x01]);
        b.commit().unwrap();

        let mut found: Vec<String> = RepoInfo::scan_out_of_scope(&eng)
            .map(|r| r.unwrap().as_str().to_string())
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec![plc('a').as_str().to_string(), plc('c').as_str().to_string()]
        );
    }

    #[test]
    fn repo_info_round_trip_with_identity_interns_host() {
        let eng = MemEngine::new();
        let reg = registry();
        let info = sample_info(h("example.com"));
        let mut b = eng.batch();
        info.store(&Did::raw("did:plc:abc"), &mut b);
        b.commit().unwrap();

        let got = RepoInfo::load(&eng, &reg, &Did::raw("did:plc:abc"))
            .unwrap()
            .expect("entry present");
        let id = got.identity;
        assert_eq!(id.pds_host.name().as_str(), "example.com");
        assert_eq!(id.signing_key, SigningKey::raw(vec![1, 2, 3, 4]));
        assert_eq!(&*id.supposed_handle.unwrap(), "alice.example");
        // load goes through registry — the returned Arc<Host> is the
        // registry-interned one, not the test-only Host we stored from.
        let from_reg = reg.get("example.com").unwrap();
        assert!(Arc::ptr_eq(&id.pds_host, &from_reg));
    }

    #[test]
    fn repo_info_missing_permit_field_decodes_to_none() {
        // a `None` hint is skipped on write (skip_serializing_if), so the
        // stored bytes omit the field entirely. loading must fall back to
        // `None` rather than error on the missing field — this is the guard
        // for `#[serde(default)]` on `DbRepoInfo::first_resync_needs_permit`.
        let eng = MemEngine::new();
        let reg = registry();
        let info = sample_info(h("example.com"));
        assert!(info.first_resync_needs_permit.is_none());

        let mut b = eng.batch();
        info.store(&Did::raw("did:plc:abc"), &mut b);
        b.commit().unwrap();

        let got = RepoInfo::load(&eng, &reg, &Did::raw("did:plc:abc"))
            .unwrap()
            .expect("entry present");
        assert!(got.first_resync_needs_permit.is_none());
    }

    #[test]
    fn repo_info_round_trips_needs_permit_flag() {
        let eng = MemEngine::new();
        let reg = registry();
        let mut info = sample_info(h("example.com"));
        info.first_resync_needs_permit = Some(());

        let mut b = eng.batch();
        info.store(&Did::raw("did:plc:abc"), &mut b);
        b.commit().unwrap();

        let got = RepoInfo::load(&eng, &reg, &Did::raw("did:plc:abc"))
            .unwrap()
            .expect("entry present");
        assert_eq!(got.first_resync_needs_permit, Some(()));
    }

    #[test]
    fn repo_info_round_trip_with_desync_and_resync_info() {
        let eng = MemEngine::new();
        let reg = registry();
        let host = h("example.com");
        let mut info = sample_info(host.clone());
        info.sync_status = SyncStatus::Desynchronized(Desynchronized {
            reason: DesyncReason::FirehoseFail {
                bad_commit_rev: Some(42.try_into().unwrap()),
            },
            due_at: t(5_000),
            last_resync_error: Some("bla blah".to_string()),
            resync_attempts: 3,
        });
        info.last_resync = Some(ResyncInfo::new(
            t(2_000),
            Some(1024),
            Some(100),
            Some(host),
            Some(Duration::from_millis(1500)),
        ));
        info.resyncs = 1;

        let mut b = eng.batch();
        info.store(&Did::raw("did:plc:abc"), &mut b);
        b.commit().unwrap();

        let got = RepoInfo::load(&eng, &reg, &Did::raw("did:plc:abc"))
            .unwrap()
            .expect("entry present");
        match got.sync_status {
            SyncStatus::Desynchronized(d) => {
                assert!(matches!(
                    d.reason,
                    DesyncReason::FirehoseFail {
                        bad_commit_rev: Some(_),
                    }
                ));
                // annoying to test that the tid is 42...
                assert_eq!(d.resync_attempts, 3);
                assert_eq!(d.due_at, t(5_000));
            }
            _ => panic!("expected desynchronized"),
        }
        let last = got.last_resync.expect("last_resync preserved");
        assert_eq!(last.at, t(2_000));
        assert_eq!(last.size, Some(1024));
        assert_eq!(last.count, Some(100));
        assert_eq!(last.duration(), Some(Duration::from_millis(1500)));
        // from_host is interned at load: even though we stored from a
        // non-registry Host, the loaded value is the registry's copy
        let host_via = last.host().expect("from_host preserved");
        let from_reg = reg.get("example.com").unwrap();
        assert!(Arc::ptr_eq(host_via, &from_reg));
    }

    #[test]
    fn repo_info_load_errors_on_bad_pds_hostname() {
        let eng = MemEngine::new();
        let reg = registry();
        // directly stage a DbRepoInfo with a non-bare hostname (userinfo
        // present), bypassing the in-memory `RepoInfo` shape. registry.get
        // on load should reject it.
        let db_info = DbRepoInfo {
            upstream_status: AccountStatus::Active,
            moderation: None,
            sync_status: SyncStatus::Synchronized,
            identity: DbRepoIdentity {
                pds: "user@example.com".into(),
                signing_key: SigningKey::raw(vec![]),
                supposed_handle: Some("x.example".into()),
                resolved_at: t(1000),
            },
            first_seen_at: t(0),
            resyncs: 0,
            pds_changes: 0,
            handle_changes: 0,
            last_resync: None,
            first_resync_needs_permit: false,
        };
        let bytes = drisl::to_vec(&db_info).unwrap();
        let mut b = eng.batch();
        b.put(&DbRepoInfo::key(&Did::raw("did:plc:abc")), &bytes);
        b.commit().unwrap();

        let err = RepoInfo::load(&eng, &reg, &Did::raw("did:plc:abc")).unwrap_err();
        assert!(matches!(err, LoadError::Decode(DecodeError::Hostname(_))));
    }
}
