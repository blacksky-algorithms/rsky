//! Keyed per-repo and per-host backfill state.
//!
//! This replaces the append-only `repo_backfill` queue, and it is the change
//! that makes backfill bounded rather than a function of uptime.
//!
//! The queue it replaced keyed entries `1:{timestamp_nanos}:{did}`, so the same
//! DID enqueued twice produced two distinct keys and nothing collapsed them.
//! There was no stored `rev`, so every enumeration pass re-enqueued the entire
//! network. Here the DID is the primary key and `indexed_rev` records what was
//! actually indexed, so:
//!
//! * re-enumerating is idempotent and nearly free -- unchanged repos are a
//!   no-op, not a new row;
//! * a repo is fetched only when its `rev` has moved past what we indexed;
//! * retries carry a cooldown and an attempt count instead of being re-queued
//!   behind several million other entries;
//! * inactive repos are written off without spending a `getRepo` that would
//!   400;
//! * each repo has a `source` -- the PDS host it is fetched from directly, or
//!   `hubble` -- and a direct source always wins over hubble, so the mirror is
//!   only asked for repos no mushroom will serve us.
//!
//! `SQLite`, because `rusqlite` is already a dependency and a single writer at
//! a few thousand rows a second is well within it. ~160 bytes per repo, so the
//! whole network is ~7 GB on disk.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, params};

use super::host::Hostname;
use super::source::RepoRef;

/// Attempts before a repo is written off. Matches `car-dump`'s budget.
pub const MAX_ATTEMPTS: u32 = 5;

/// The source name for the hubble mirror. Anything else is a PDS hostname.
pub const HUBBLE_SOURCE: &str = "hubble";

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("state lock poisoned")]
    Poisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoState {
    /// Wants fetching, once any cooldown has expired.
    Pending,
    /// Handed to a worker. Reset to `Pending` on startup so a crash mid-fetch
    /// does not strand the row.
    Claimed,
    /// Indexed at `indexed_rev`.
    Done,
    /// Will not be retried: out of attempts, or the account is gone.
    Terminal,
}

impl RepoState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Done => "done",
            Self::Terminal => "terminal",
        }
    }
}

/// What an enumeration upsert decided about one repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upsert {
    /// New repo, or one whose `rev` advanced past what we indexed.
    NeedsFetch,
    /// Already indexed at (or past) this `rev`.
    Unchanged,
    /// Inactive upstream, or already written off.
    Skipped,
    /// A direct PDS source already owns this repo; hubble yields to it.
    Deferred,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub pending: u64,
    pub claimed: u64,
    pub done: u64,
    pub terminal: u64,
}

impl Stats {
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.pending + self.claimed + self.done + self.terminal
    }
}

/// Where enumeration of one host stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListState {
    Pending,
    Done,
    /// `listRepos` failed terminally: the host cannot be enumerated.
    Unreachable,
}

impl ListState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Done => "done",
            Self::Unreachable => "unreachable",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "done" => Self::Done,
            "unreachable" => Self::Unreachable,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRow {
    pub host: Hostname,
    pub account_count: u64,
    pub status: Option<String>,
    /// Fetch directly from this host rather than through hubble.
    pub direct: bool,
    pub list_state: ListState,
    /// Persisted reduced fetch concurrency, so a restart does not hammer a
    /// struggling host at full blast again.
    pub concurrency: Option<usize>,
    pub cooldown_until: i64,
}

/// A batch of work claimed for one source.
pub type Claimed = Vec<(String, String)>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS repo (
    did             TEXT PRIMARY KEY,
    source          TEXT NOT NULL,
    rev             TEXT NOT NULL DEFAULT '',
    indexed_rev     TEXT,
    state           TEXT NOT NULL,
    attempts        INTEGER NOT NULL DEFAULT 0,
    cooldown_until  INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    updated_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS repo_claimable ON repo (source, state, cooldown_until);
CREATE INDEX IF NOT EXISTS repo_state ON repo (state);

CREATE TABLE IF NOT EXISTS host (
    name            TEXT PRIMARY KEY,
    account_count   INTEGER NOT NULL DEFAULT 0,
    status          TEXT,
    direct          INTEGER NOT NULL DEFAULT 0,
    list_state      TEXT NOT NULL DEFAULT 'pending',
    concurrency     INTEGER,
    cooldown_until  INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    updated_at      INTEGER NOT NULL
);

-- Cursors are TEXT: relay cursors are integers, hubble's are DIDs.
CREATE TABLE IF NOT EXISTS cursor (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);
";

#[derive(Clone)]
pub struct RepoStateStore {
    conn: Arc<Mutex<Connection>>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Whether `indexed` is at or past `listed`. Revs are TIDs, which sort
/// lexicographically when well-formed (13 chars); anything else compares by
/// equality only.
#[must_use]
pub fn rev_covers(indexed: &str, listed: &str) -> bool {
    if listed.is_empty() {
        return false;
    }
    if indexed.len() == 13 && listed.len() == 13 {
        indexed >= listed
    } else {
        indexed == listed
    }
}

impl RepoStateStore {
    pub fn open(path: &Path) -> Result<Self, StateError> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, StateError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StateError> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;",
        )?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StateError> {
        self.conn.lock().map_err(|_| StateError::Poisoned)
    }

    // ---------------------------------------------------------------- repos

    /// Record what enumeration saw. Returns whether this repo now wants
    /// fetching.
    pub fn upsert(
        &self,
        repo: &RepoRef,
        source: &str,
        unfetchable: bool,
    ) -> Result<Upsert, StateError> {
        let conn = self.lock()?;
        let outcome = Self::upsert_on(&conn, repo, source, unfetchable);
        drop(conn);
        outcome
    }

    /// Upsert a whole page in one transaction. Returns
    /// `(needs_fetch, unchanged, skipped, deferred)`.
    pub fn upsert_batch(
        &self,
        repos: &[RepoRef],
        source: &str,
        unfetchable: &dyn Fn(&RepoRef) -> bool,
    ) -> Result<(u64, u64, u64, u64), StateError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let (mut fetch, mut unchanged, mut skipped, mut deferred) = (0u64, 0u64, 0u64, 0u64);
        for repo in repos {
            match Self::upsert_on(&tx, repo, source, unfetchable(repo))? {
                Upsert::NeedsFetch => fetch += 1,
                Upsert::Unchanged => unchanged += 1,
                Upsert::Skipped => skipped += 1,
                Upsert::Deferred => deferred += 1,
            }
        }
        tx.commit()?;
        drop(conn);
        Ok((fetch, unchanged, skipped, deferred))
    }

    fn upsert_on(
        conn: &Connection,
        repo: &RepoRef,
        source: &str,
        unfetchable: bool,
    ) -> Result<Upsert, StateError> {
        let existing: Option<(String, Option<String>, i64, String, String)> = conn
            .query_row(
                "SELECT state, indexed_rev, attempts, rev, source FROM repo WHERE did = ?1",
                params![repo.did],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;

        if let Some((state, indexed_rev, attempts, stored_rev, stored_source)) = &existing {
            // A direct PDS source owns this repo. hubble's enumeration says
            // nothing new about it, whatever its rev: the mushroom is the
            // authority and the cheaper pipe.
            if source == HUBBLE_SOURCE
                && stored_source != HUBBLE_SOURCE
                && state != RepoState::Terminal.as_str()
            {
                return Ok(Upsert::Deferred);
            }
            if unfetchable {
                // Fall through to the write below.
            } else {
                // The skip that makes re-enumeration cheap: already indexed at
                // or past this rev.
                if let Some(idx) = indexed_rev {
                    if rev_covers(idx, &repo.rev) {
                        return Ok(Upsert::Unchanged);
                    }
                }
                // A repo written off for repeated failure stays written off
                // until its rev moves -- retrying it would fail the same way.
                //
                // A repo written off for being inactive upstream has
                // `attempts` of 0 and gets another try as soon as enumeration
                // reports it active again, even at the same rev: a reactivated
                // account usually has not gained records.
                if state == RepoState::Terminal.as_str()
                    && indexed_rev.is_none()
                    && *attempts > 0
                    && *stored_rev == repo.rev
                {
                    return Ok(Upsert::Skipped);
                }
            }
        }

        if unfetchable {
            conn.execute(
                "INSERT INTO repo (did, source, rev, state, attempts, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5)
                 ON CONFLICT(did) DO UPDATE SET
                     rev = excluded.rev, source = excluded.source,
                     state = excluded.state, updated_at = excluded.updated_at",
                params![
                    repo.did,
                    source,
                    repo.rev,
                    RepoState::Terminal.as_str(),
                    now()
                ],
            )?;
            return Ok(Upsert::Skipped);
        }

        conn.execute(
            "INSERT INTO repo (did, source, rev, state, attempts, cooldown_until, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, 0, ?5)
             ON CONFLICT(did) DO UPDATE SET
                 rev            = excluded.rev,
                 source         = excluded.source,
                 state          = excluded.state,
                 attempts       = 0,
                 cooldown_until = 0,
                 updated_at     = excluded.updated_at",
            params![
                repo.did,
                source,
                repo.rev,
                RepoState::Pending.as_str(),
                now()
            ],
        )?;
        Ok(Upsert::NeedsFetch)
    }

    /// Take up to `limit` repos of one source that are ready to fetch, marking
    /// them claimed. Ordered by `cooldown_until` so a cooled-down row is not
    /// starved behind newly enumerated ones; that is the index order, so a
    /// claim never sorts the source's whole backlog.
    pub fn claim_for_source(&self, source: &str, limit: usize) -> Result<Claimed, StateError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let rows: Claimed = {
            let mut stmt = tx.prepare_cached(
                "SELECT did, rev FROM repo
                  WHERE source = ?1 AND state = ?2 AND cooldown_until <= ?3
                  ORDER BY cooldown_until
                  LIMIT ?4",
            )?;
            let iter = stmt.query_map(
                params![
                    source,
                    RepoState::Pending.as_str(),
                    now(),
                    i64::try_from(limit).unwrap_or(i64::MAX)
                ],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            iter.collect::<Result<_, _>>()?
        };
        {
            let mut stmt =
                tx.prepare_cached("UPDATE repo SET state = ?2, updated_at = ?3 WHERE did = ?1")?;
            for (did, _) in &rows {
                stmt.execute(params![did, RepoState::Claimed.as_str(), now()])?;
            }
        }
        tx.commit()?;
        drop(conn);
        Ok(rows)
    }

    /// Whether one source has at least one claimable row. An index probe, so
    /// the coordinator can ask this for every host every few seconds.
    pub fn has_claimable(&self, source: &str) -> Result<bool, StateError> {
        let conn = self.lock()?;
        let found = conn
            .query_row(
                "SELECT 1 FROM repo
                  WHERE source = ?1 AND state = ?2 AND cooldown_until <= ?3
                  LIMIT 1",
                params![source, RepoState::Pending.as_str(), now()],
                |_| Ok(()),
            )
            .optional()?;
        drop(conn);
        Ok(found.is_some())
    }

    /// Sources that have claimable work right now, with how much. A full scan
    /// of pending rows: for status output, not the hot path.
    pub fn pending_sources(&self) -> Result<Vec<(String, u64)>, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare_cached(
            "SELECT source, COUNT(*) FROM repo
              WHERE state = ?1 AND cooldown_until <= ?2
              GROUP BY source ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt
            .query_map(params![RepoState::Pending.as_str(), now()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .map(|r| r.map(|(s, n)| (s, u64::try_from(n).unwrap_or(0))))
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        Ok(rows)
    }

    /// Mark a repo indexed at `rev` -- the rev of the archive that was written,
    /// not the rev seen at enumeration.
    pub fn complete(&self, did: &str, rev: &str) -> Result<(), StateError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE repo SET state = ?2, indexed_rev = ?3, attempts = 0,
                             cooldown_until = 0, last_error = NULL, updated_at = ?4
              WHERE did = ?1",
            params![did, RepoState::Done.as_str(), rev, now()],
        )?;
        drop(conn);
        Ok(())
    }

    /// Record a failed attempt.
    ///
    /// A transient failure goes back to pending with a cooldown until the
    /// attempt budget runs out. Once it is out, or the failure was terminal, the
    /// repo is handed to `fallback` (hubble, for a repo its own PDS would not
    /// serve) with a fresh budget -- or written off if there is none.
    pub fn fail(
        &self,
        did: &str,
        transient: bool,
        cooldown_secs: u64,
        err: &str,
        fallback: Option<&str>,
    ) -> Result<RepoState, StateError> {
        let conn = self.lock()?;
        let (attempts, source): (u32, String) = conn
            .query_row(
                "SELECT attempts, source FROM repo WHERE did = ?1",
                params![did],
                |r| Ok((r.get::<_, i64>(0)?, r.get(1)?)),
            )
            .optional()?
            .map_or((0, String::new()), |(a, s)| {
                (u32::try_from(a).unwrap_or(0), s)
            });
        let attempt_count = attempts.saturating_add(1);

        if transient && attempt_count < MAX_ATTEMPTS {
            let cooldown = now().saturating_add(i64::try_from(cooldown_secs).unwrap_or(i64::MAX));
            conn.execute(
                "UPDATE repo SET state = ?2, attempts = ?3, cooldown_until = ?4,
                                 last_error = ?5, updated_at = ?6
                  WHERE did = ?1",
                params![
                    did,
                    RepoState::Pending.as_str(),
                    i64::from(attempt_count),
                    cooldown,
                    err,
                    now()
                ],
            )?;
            drop(conn);
            return Ok(RepoState::Pending);
        }

        if let Some(fb) = fallback {
            if fb != source {
                conn.execute(
                    "UPDATE repo SET source = ?2, state = ?3, attempts = 0, cooldown_until = 0,
                                     last_error = ?4, updated_at = ?5
                      WHERE did = ?1",
                    params![did, fb, RepoState::Pending.as_str(), err, now()],
                )?;
                drop(conn);
                return Ok(RepoState::Pending);
            }
        }

        conn.execute(
            "UPDATE repo SET state = ?2, attempts = ?3, cooldown_until = 0,
                             last_error = ?4, updated_at = ?5
              WHERE did = ?1",
            params![
                did,
                RepoState::Terminal.as_str(),
                i64::from(attempt_count),
                err,
                now()
            ],
        )?;
        drop(conn);
        Ok(RepoState::Terminal)
    }

    /// Return every claimed row to pending. Called at startup: a claim is
    /// in-memory work, so anything still claimed is from a process that died.
    pub fn reset_claimed(&self) -> Result<u64, StateError> {
        let conn = self.lock()?;
        let n = conn.execute(
            "UPDATE repo SET state = ?1, updated_at = ?2 WHERE state = ?3",
            params![
                RepoState::Pending.as_str(),
                now(),
                RepoState::Claimed.as_str()
            ],
        )?;
        drop(conn);
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// The source a repo is currently assigned to, if known.
    pub fn source_of(&self, did: &str) -> Result<Option<String>, StateError> {
        let conn = self.lock()?;
        let s = conn
            .query_row(
                "SELECT source FROM repo WHERE did = ?1",
                params![did],
                |r| r.get(0),
            )
            .optional()?;
        drop(conn);
        Ok(s)
    }

    pub fn stats(&self) -> Result<Stats, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT state, COUNT(*) FROM repo GROUP BY state")?;
        let mut stats = Stats::default();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (state_name, count) = row?;
            let n = count.try_into().unwrap_or(0);
            match state_name.as_str() {
                "pending" => stats.pending = n,
                "claimed" => stats.claimed = n,
                "done" => stats.done = n,
                "terminal" => stats.terminal = n,
                _ => {}
            }
        }
        drop(stmt);
        drop(conn);
        Ok(stats)
    }

    /// Per-source `(source, pending, done, terminal)`, largest backlog first.
    pub fn stats_by_source(&self) -> Result<Vec<(String, u64, u64, u64)>, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT source,
                    SUM(state IN ('pending','claimed')),
                    SUM(state = 'done'),
                    SUM(state = 'terminal')
               FROM repo GROUP BY source ORDER BY 2 DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .map(|r| {
                r.map(|(s, p, d, t)| {
                    (
                        s,
                        u64::try_from(p).unwrap_or(0),
                        u64::try_from(d).unwrap_or(0),
                        u64::try_from(t).unwrap_or(0),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        Ok(rows)
    }

    // ---------------------------------------------------------------- hosts

    /// Record a discovered host. Discovery metadata is refreshed every time;
    /// enumeration progress and the adaptive concurrency floor are kept.
    pub fn upsert_host(
        &self,
        host: &Hostname,
        account_count: u64,
        status: Option<&str>,
        direct: bool,
    ) -> Result<(), StateError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO host (name, account_count, status, direct, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(name) DO UPDATE SET
                 account_count = excluded.account_count,
                 status        = excluded.status,
                 direct        = excluded.direct,
                 updated_at    = excluded.updated_at",
            params![
                host.as_str(),
                i64::try_from(account_count).unwrap_or(i64::MAX),
                status,
                i32::from(direct),
                now()
            ],
        )?;
        drop(conn);
        Ok(())
    }

    /// Hosts flagged for direct fetching, largest first.
    pub fn direct_hosts(&self) -> Result<Vec<HostRow>, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT name, account_count, status, direct, list_state, concurrency, cooldown_until
               FROM host WHERE direct = 1 ORDER BY account_count DESC, name",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        Ok(rows
            .into_iter()
            .filter_map(
                |(name, count, status, direct, list_state, conc, cooldown)| {
                    Hostname::new(&name).ok().map(|host| HostRow {
                        host,
                        account_count: u64::try_from(count).unwrap_or(0),
                        status,
                        direct: direct != 0,
                        list_state: ListState::parse(&list_state),
                        concurrency: conc.and_then(|c| usize::try_from(c).ok()),
                        cooldown_until: cooldown,
                    })
                },
            )
            .collect())
    }

    pub fn host(&self, host: &Hostname) -> Result<Option<HostRow>, StateError> {
        Ok(self.direct_hosts()?.into_iter().find(|h| &h.host == host))
    }

    pub fn set_list_state(&self, host: &Hostname, state: ListState) -> Result<(), StateError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE host SET list_state = ?2, updated_at = ?3 WHERE name = ?1",
            params![host.as_str(), state.as_str(), now()],
        )?;
        drop(conn);
        Ok(())
    }

    /// Persist a reduced fetch concurrency. Only ever lowers what is stored:
    /// recovery is in-memory, so a restart warms up from the floor.
    pub fn set_host_concurrency(&self, host: &Hostname, limit: usize) -> Result<(), StateError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE host SET concurrency = MIN(COALESCE(concurrency, ?2), ?2), updated_at = ?3
              WHERE name = ?1",
            params![host.as_str(), i64::try_from(limit).unwrap_or(1), now()],
        )?;
        drop(conn);
        Ok(())
    }

    pub fn set_host_cooldown(
        &self,
        host: &Hostname,
        cooldown_secs: u64,
        err: &str,
    ) -> Result<(), StateError> {
        let conn = self.lock()?;
        let until = now().saturating_add(i64::try_from(cooldown_secs).unwrap_or(i64::MAX));
        conn.execute(
            "UPDATE host SET cooldown_until = ?2, last_error = ?3, updated_at = ?4 WHERE name = ?1",
            params![host.as_str(), until, err, now()],
        )?;
        drop(conn);
        Ok(())
    }

    /// Start a fresh enumeration campaign: every host re-lists from scratch.
    /// Indexed revs are kept, so unchanged repos are not re-fetched.
    pub fn reset_enumeration(&self) -> Result<(), StateError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE host SET list_state = 'pending', updated_at = ?1 WHERE list_state = 'done'",
            params![now()],
        )?;
        conn.execute("DELETE FROM cursor WHERE key LIKE 'enum:%'", [])?;
        drop(conn);
        Ok(())
    }

    // -------------------------------------------------------------- cursors

    pub fn get_cursor(&self, key: &str) -> Result<Option<String>, StateError> {
        let conn = self.lock()?;
        let value = conn
            .query_row(
                "SELECT value FROM cursor WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?;
        drop(conn);
        Ok(value)
    }

    pub fn set_cursor(&self, key: &str, value: &str) -> Result<(), StateError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO cursor (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                            updated_at = excluded.updated_at",
            params![key, value, now()],
        )?;
        drop(conn);
        Ok(())
    }

    pub fn clear_cursor(&self, key: &str) -> Result<(), StateError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM cursor WHERE key = ?1", params![key])?;
        drop(conn);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(did: &str, rev: &str) -> RepoRef {
        RepoRef {
            did: did.into(),
            rev: rev.into(),
            active: true,
            status: None,
        }
    }

    fn store() -> RepoStateStore {
        RepoStateStore::open_in_memory().unwrap()
    }

    const HOST: &str = "morel.us-east.host.bsky.network";

    #[test]
    fn a_new_repo_needs_fetching() {
        let s = store();
        assert_eq!(
            s.upsert(&repo("did:a", "r1"), HOST, false).unwrap(),
            Upsert::NeedsFetch
        );
        assert_eq!(s.stats().unwrap().pending, 1);
        assert_eq!(s.source_of("did:a").unwrap().as_deref(), Some(HOST));
        assert_eq!(s.source_of("did:zzz").unwrap(), None);
    }

    #[test]
    fn re_enumeration_of_an_indexed_repo_is_a_no_op() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        let claimed = s.claim_for_source(HOST, 10).unwrap();
        assert_eq!(claimed, vec![("did:a".to_owned(), "r1".to_owned())]);
        s.complete("did:a", "r1").unwrap();
        assert_eq!(
            s.upsert(&repo("did:a", "r1"), HOST, false).unwrap(),
            Upsert::Unchanged
        );
        assert!(s.claim_for_source(HOST, 10).unwrap().is_empty());
        let st = s.stats().unwrap();
        assert_eq!((st.done, st.pending, st.total()), (1, 0, 1));
    }

    #[test]
    fn an_advanced_rev_makes_it_fetchable_again() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        s.complete("did:a", "r1").unwrap();
        assert_eq!(
            s.upsert(&repo("did:a", "r2"), HOST, false).unwrap(),
            Upsert::NeedsFetch
        );
        assert_eq!(s.stats().unwrap().pending, 1);
    }

    #[test]
    fn tid_revs_compare_by_order_so_a_stale_listing_is_unchanged() {
        // 13-char TIDs sort lexicographically.
        assert!(rev_covers("3lz7gd2xq5c2a", "3lz7gd2xq5c2a"));
        assert!(rev_covers("3lz7gd2xq5c2b", "3lz7gd2xq5c2a"));
        assert!(!rev_covers("3lz7gd2xq5c2a", "3lz7gd2xq5c2b"));
        assert!(!rev_covers("anything", ""), "an empty listing never covers");
        assert!(rev_covers("r1", "r1"));
        assert!(!rev_covers("r2", "r1"), "non-TIDs compare by equality only");

        let s = store();
        s.upsert(&repo("did:a", "3lz7gd2xq5c2a"), HOST, false)
            .unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        // The archive that came back was newer than the listing.
        s.complete("did:a", "3lz7gd2xq5c2c").unwrap();
        assert_eq!(
            s.upsert(&repo("did:a", "3lz7gd2xq5c2b"), HOST, false)
                .unwrap(),
            Upsert::Unchanged,
            "a listing behind what we indexed creates no work"
        );
    }

    #[test]
    fn inactive_repos_are_written_off_without_a_fetch() {
        let s = store();
        assert_eq!(
            s.upsert(&repo("did:a", "r1"), HOST, true).unwrap(),
            Upsert::Skipped
        );
        assert!(s.claim_for_source(HOST, 10).unwrap().is_empty());
        assert_eq!(s.stats().unwrap().terminal, 1);
    }

    #[test]
    fn a_reactivated_account_becomes_fetchable_again() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, true).unwrap();
        assert_eq!(
            s.upsert(&repo("did:a", "r1"), HOST, false).unwrap(),
            Upsert::NeedsFetch,
            "same rev, but inactive->active is a reason to fetch"
        );
    }

    #[test]
    fn a_repo_written_off_for_failures_stays_written_off_at_the_same_rev() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        assert_eq!(
            s.fail("did:a", false, 0, "404", None).unwrap(),
            RepoState::Terminal
        );
        assert_eq!(
            s.upsert(&repo("did:a", "r1"), HOST, false).unwrap(),
            Upsert::Skipped
        );
        assert_eq!(
            s.upsert(&repo("did:a", "r2"), HOST, false).unwrap(),
            Upsert::NeedsFetch,
            "a moved rev earns another try"
        );
    }

    #[test]
    fn an_empty_rev_never_counts_as_unchanged() {
        let s = store();
        s.upsert(&repo("did:a", ""), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        s.complete("did:a", "").unwrap();
        assert_eq!(
            s.upsert(&repo("did:a", ""), HOST, false).unwrap(),
            Upsert::NeedsFetch,
            "relay listings without a rev cannot prove anything"
        );
    }

    #[test]
    fn transient_failures_retry_with_cooldown_then_give_up() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        for attempt in 1..MAX_ATTEMPTS {
            s.claim_for_source(HOST, 10).unwrap();
            assert_eq!(
                s.fail("did:a", true, 0, "503", None).unwrap(),
                RepoState::Pending,
                "attempt {attempt}"
            );
        }
        s.claim_for_source(HOST, 10).unwrap();
        assert_eq!(
            s.fail("did:a", true, 0, "503", None).unwrap(),
            RepoState::Terminal
        );
        assert_eq!(s.stats().unwrap().terminal, 1);
    }

    #[test]
    fn exhausted_direct_fetches_fall_back_to_hubble_with_a_fresh_budget() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            s.claim_for_source(HOST, 10).unwrap();
            s.fail("did:a", true, 0, "503", Some(HUBBLE_SOURCE))
                .unwrap();
        }
        assert_eq!(
            s.source_of("did:a").unwrap().as_deref(),
            Some(HUBBLE_SOURCE)
        );
        assert!(s.claim_for_source(HOST, 10).unwrap().is_empty());
        let claimed = s.claim_for_source(HUBBLE_SOURCE, 10).unwrap();
        assert_eq!(
            claimed.len(),
            1,
            "hubble now owns it, pending, attempts reset"
        );

        // A terminal failure on the direct host also hands over immediately.
        s.upsert(&repo("did:b", "r1"), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        assert_eq!(
            s.fail("did:b", false, 0, "400 RepoNotFound", Some(HUBBLE_SOURCE))
                .unwrap(),
            RepoState::Pending
        );
        assert_eq!(
            s.source_of("did:b").unwrap().as_deref(),
            Some(HUBBLE_SOURCE)
        );

        // But hubble failing terminally has nowhere to go.
        s.claim_for_source(HUBBLE_SOURCE, 10).unwrap();
        assert_eq!(
            s.fail("did:b", false, 0, "400", Some(HUBBLE_SOURCE))
                .unwrap(),
            RepoState::Terminal
        );
    }

    #[test]
    fn hubble_yields_to_a_direct_source_and_a_direct_source_takes_over_from_hubble() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        assert_eq!(
            s.upsert(&repo("did:a", "r9"), HUBBLE_SOURCE, false)
                .unwrap(),
            Upsert::Deferred,
            "hubble does not steal a pending direct repo, even at a newer rev"
        );
        assert_eq!(s.source_of("did:a").unwrap().as_deref(), Some(HOST));

        s.upsert(&repo("did:b", "r1"), HUBBLE_SOURCE, false)
            .unwrap();
        assert_eq!(
            s.upsert(&repo("did:b", "r1"), HOST, false).unwrap(),
            Upsert::NeedsFetch,
            "a direct listing takes a pending hubble repo"
        );
        assert_eq!(s.source_of("did:b").unwrap().as_deref(), Some(HOST));

        // Once indexed, ownership does not matter: unchanged is unchanged.
        s.claim_for_source(HOST, 10).unwrap();
        s.complete("did:b", "r1").unwrap();
        assert_eq!(
            s.upsert(&repo("did:b", "r1"), HUBBLE_SOURCE, false)
                .unwrap(),
            Upsert::Deferred
        );

        // A terminal direct row is fair game for hubble.
        s.upsert(&repo("did:c", "r1"), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        s.fail("did:c", false, 0, "400", None).unwrap();
        assert_eq!(
            s.upsert(&repo("did:c", "r2"), HUBBLE_SOURCE, false)
                .unwrap(),
            Upsert::NeedsFetch
        );
        assert_eq!(
            s.source_of("did:c").unwrap().as_deref(),
            Some(HUBBLE_SOURCE)
        );
    }

    #[test]
    fn a_cooldown_hides_the_row_until_it_expires() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        s.fail("did:a", true, 3600, "429", None).unwrap();
        assert!(s.claim_for_source(HOST, 10).unwrap().is_empty());
        assert!(s.pending_sources().unwrap().is_empty());
        assert_eq!(s.stats().unwrap().pending, 1, "still pending, just parked");
    }

    #[test]
    fn claimed_rows_are_recovered_after_a_crash() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        assert_eq!(s.stats().unwrap().claimed, 1);
        assert_eq!(s.reset_claimed().unwrap(), 1);
        assert_eq!(s.stats().unwrap().pending, 1);
        assert_eq!(s.reset_claimed().unwrap(), 0);
    }

    #[test]
    fn claims_are_exclusive_and_per_source() {
        let s = store();
        for i in 0..5 {
            s.upsert(&repo(&format!("did:{i}"), "r1"), HOST, false)
                .unwrap();
        }
        s.upsert(&repo("did:h", "r1"), HUBBLE_SOURCE, false)
            .unwrap();
        let a = s.claim_for_source(HOST, 3).unwrap();
        let b = s.claim_for_source(HOST, 3).unwrap();
        assert_eq!((a.len(), b.len()), (3, 2));
        assert!(a.iter().all(|x| !b.contains(x)));
        assert!(s.claim_for_source(HOST, 3).unwrap().is_empty());
        assert_eq!(s.claim_for_source(HUBBLE_SOURCE, 3).unwrap().len(), 1);
    }

    #[test]
    fn has_claimable_is_per_source_and_respects_cooldowns() {
        let s = store();
        assert!(!s.has_claimable(HOST).unwrap());
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        assert!(s.has_claimable(HOST).unwrap());
        assert!(!s.has_claimable(HUBBLE_SOURCE).unwrap());
        s.claim_for_source(HOST, 10).unwrap();
        assert!(!s.has_claimable(HOST).unwrap(), "claimed is not claimable");
        s.fail("did:a", true, 3600, "429", None).unwrap();
        assert!(!s.has_claimable(HOST).unwrap(), "cooling down");
    }

    #[test]
    fn pending_sources_report_backlog_largest_first() {
        let s = store();
        for i in 0..3 {
            s.upsert(&repo(&format!("did:{i}"), "r1"), HOST, false)
                .unwrap();
        }
        s.upsert(&repo("did:h", "r1"), HUBBLE_SOURCE, false)
            .unwrap();
        let p = s.pending_sources().unwrap();
        assert_eq!(p, vec![(HOST.to_owned(), 3), (HUBBLE_SOURCE.to_owned(), 1)]);
        let by = s.stats_by_source().unwrap();
        assert_eq!(by[0], (HOST.to_owned(), 3, 0, 0));
    }

    #[test]
    fn a_batch_reports_what_it_did() {
        let s = store();
        s.upsert(&repo("did:done", "r1"), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        s.complete("did:done", "r1").unwrap();
        s.upsert(&repo("did:direct", "r1"), HOST, false).unwrap();

        let page = vec![
            repo("did:done", "r1"),
            repo("did:new", "r1"),
            RepoRef {
                did: "did:gone".into(),
                rev: "r1".into(),
                active: false,
                status: Some("deleted".into()),
            },
            repo("did:direct", "r1"),
        ];
        let (fetch, unchanged, skipped, deferred) = s
            .upsert_batch(&page, HUBBLE_SOURCE, &|r| !r.active)
            .unwrap();
        assert_eq!((fetch, unchanged, skipped, deferred), (1, 0, 1, 2));
        assert_eq!(s.stats().unwrap().total(), 4);
    }

    #[test]
    fn hosts_round_trip_with_progress_and_floors() {
        let s = store();
        let h = Hostname::new(HOST).unwrap();
        s.upsert_host(&h, 100, Some("active"), true).unwrap();
        s.upsert_host(&Hostname::new("tiny.example").unwrap(), 1, None, false)
            .unwrap();
        let direct = s.direct_hosts().unwrap();
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].list_state, ListState::Pending);
        assert_eq!(direct[0].concurrency, None);
        assert!(direct[0].direct);

        s.set_list_state(&h, ListState::Done).unwrap();
        s.set_host_concurrency(&h, 5).unwrap();
        s.set_host_concurrency(&h, 7).unwrap();
        s.set_host_cooldown(&h, 60, "429").unwrap();
        // Rediscovery refreshes metadata but keeps progress and the floor.
        s.upsert_host(&h, 200, Some("idle"), true).unwrap();
        let row = s.host(&h).unwrap().unwrap();
        assert_eq!(row.account_count, 200);
        assert_eq!(row.status.as_deref(), Some("idle"));
        assert_eq!(row.list_state, ListState::Done);
        assert_eq!(row.concurrency, Some(5), "only ever lowers");
        assert!(row.cooldown_until > now());

        s.set_list_state(&h, ListState::Unreachable).unwrap();
        assert_eq!(
            s.host(&h).unwrap().unwrap().list_state,
            ListState::Unreachable
        );
        assert_eq!(
            s.host(&Hostname::new("nope.example").unwrap()).unwrap(),
            None
        );
    }

    #[test]
    fn reset_enumeration_relists_done_hosts_but_keeps_indexed_revs() {
        let s = store();
        let h = Hostname::new(HOST).unwrap();
        s.upsert_host(&h, 100, None, true).unwrap();
        s.set_list_state(&h, ListState::Done).unwrap();
        s.set_cursor("enum:hubble", "did:plc:x").unwrap();
        s.set_cursor("other", "keep").unwrap();
        s.upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        s.claim_for_source(HOST, 10).unwrap();
        s.complete("did:a", "r1").unwrap();

        s.reset_enumeration().unwrap();
        assert_eq!(s.host(&h).unwrap().unwrap().list_state, ListState::Pending);
        assert_eq!(s.get_cursor("enum:hubble").unwrap(), None);
        assert_eq!(s.get_cursor("other").unwrap().as_deref(), Some("keep"));
        assert_eq!(
            s.upsert(&repo("did:a", "r1"), HOST, false).unwrap(),
            Upsert::Unchanged
        );
    }

    #[test]
    fn cursors_round_trip_as_text() {
        let s = store();
        assert_eq!(s.get_cursor("k").unwrap(), None);
        s.set_cursor("k", "did:plc:222rpxpbdd4cd2opywsqpxtz")
            .unwrap();
        assert_eq!(
            s.get_cursor("k").unwrap().as_deref(),
            Some("did:plc:222rpxpbdd4cd2opywsqpxtz")
        );
        s.set_cursor("k", "123456789").unwrap();
        assert_eq!(s.get_cursor("k").unwrap().as_deref(), Some("123456789"));
        s.clear_cursor("k").unwrap();
        assert_eq!(s.get_cursor("k").unwrap(), None);
    }

    #[test]
    fn state_names_are_stable() {
        assert_eq!(RepoState::Pending.as_str(), "pending");
        assert_eq!(RepoState::Claimed.as_str(), "claimed");
        assert_eq!(RepoState::Done.as_str(), "done");
        assert_eq!(RepoState::Terminal.as_str(), "terminal");
        assert_eq!(ListState::parse("done"), ListState::Done);
        assert_eq!(ListState::parse("weird"), ListState::Pending);
        assert_eq!(ListState::Unreachable.as_str(), "unreachable");
    }
}
