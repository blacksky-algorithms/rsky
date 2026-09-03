//! Keyed per-repo backfill state.
//!
//! This replaces the append-only `repo_backfill` queue, and it is the change
//! that makes backfill bounded rather than a function of uptime.
//!
//! The queue it replaces keys entries `1:{timestamp_nanos}:{did}`, so the same
//! DID enqueued twice produces two distinct keys and nothing anywhere collapses
//! them. There is no stored `rev`, so a second enumeration pass re-enqueues the
//! entire network. On the production appview that partition is 946 MB of
//! tombstones holding 430 live rows.
//!
//! Here the DID is the primary key and `indexed_rev` records what we actually
//! indexed, so:
//!
//! * re-enumerating is idempotent and nearly free -- unchanged repos are a
//!   no-op UPDATE, not a new row;
//! * a repo is fetched only when its `rev` has moved past what we indexed;
//! * retries carry a cooldown and an attempt count instead of being re-queued
//!   behind several million other entries;
//! * inactive repos are written off without spending a `getRepo` that would
//!   400.
//!
//! `SQLite`, because `rusqlite` is already a dependency and this needs no
//! migration to prototype. Production would likely put it in Postgres beside
//! `sub_state`; the API here is deliberately narrow enough to swap.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, params};

use super::source::RepoRef;

/// Attempts before a repo is written off. Matches `car-dump`'s budget.
pub const MAX_ATTEMPTS: u32 = 5;

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
    /// Already indexed at this exact `rev`. This is the case that makes
    /// re-enumeration cheap, and the one the current producer cannot express.
    Unchanged,
    /// Inactive upstream, or already written off.
    Skipped,
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
CREATE INDEX IF NOT EXISTS repo_claimable ON repo (state, cooldown_until);

-- Cursors are TEXT. The existing producer stores them in an i64 column via
-- `parse::<i64>().unwrap_or(0)`, which silently truncates hubble's DID cursors
-- to 0 and re-enumerates from the start of the keyspace on every restart.
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

    /// Record what enumeration saw. Returns whether this repo now wants fetching.
    ///
    /// `unfetchable` lets the caller write off accounts that upstream reports as
    /// deleted or taken down, instead of discovering it one `getRepo` 400 at a
    /// time.
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

    /// Upsert a whole page in one transaction. Enumeration is the hot path --
    /// 41.5 M repos a thousand at a time -- so this is what the runner calls.
    pub fn upsert_batch(
        &self,
        repos: &[RepoRef],
        source: &str,
        unfetchable: &dyn Fn(&RepoRef) -> bool,
    ) -> Result<(u64, u64, u64), StateError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let (mut fetch, mut unchanged, mut skipped) = (0u64, 0u64, 0u64);
        for repo in repos {
            match Self::upsert_on(&tx, repo, source, unfetchable(repo))? {
                Upsert::NeedsFetch => fetch += 1,
                Upsert::Unchanged => unchanged += 1,
                Upsert::Skipped => skipped += 1,
            }
        }
        tx.commit()?;
        drop(conn);
        Ok((fetch, unchanged, skipped))
    }

    fn upsert_on(
        conn: &Connection,
        repo: &RepoRef,
        source: &str,
        unfetchable: bool,
    ) -> Result<Upsert, StateError> {
        let existing: Option<(String, Option<String>, i64, String)> = conn
            .query_row(
                "SELECT state, indexed_rev, attempts, rev FROM repo WHERE did = ?1",
                params![repo.did],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;

        if unfetchable {
            conn.execute(
                "INSERT INTO repo (did, source, rev, state, attempts, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5)
                 ON CONFLICT(did) DO UPDATE SET
                     rev = excluded.rev, state = excluded.state, updated_at = excluded.updated_at",
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

        if let Some((state, indexed_rev, attempts, stored_rev)) = existing {
            // The skip that makes re-enumeration cheap: same rev, already indexed.
            if !repo.rev.is_empty() && indexed_rev.as_deref() == Some(repo.rev.as_str()) {
                return Ok(Upsert::Unchanged);
            }
            // A repo written off for repeated failure stays written off until
            // its rev moves -- retrying it would fail the same way.
            //
            // A repo written off for being inactive upstream has `attempts` of
            // 0, and gets another try as soon as enumeration reports it active
            // again, even at the same rev: a reactivated account usually has
            // not gained records, so waiting for its rev to move would leave it
            // written off indefinitely.
            if state == RepoState::Terminal.as_str()
                && indexed_rev.is_none()
                && attempts > 0
                && stored_rev == repo.rev
            {
                return Ok(Upsert::Skipped);
            }
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

    /// Take up to `limit` repos that are ready to fetch, marking them claimed.
    ///
    /// Ordering is by `cooldown_until` then `did` so a cooled-down row is not
    /// starved behind newly enumerated ones -- unlike the timestamp-keyed queue,
    /// where a retry is re-enqueued at the tail behind everything.
    pub fn claim(&self, limit: usize) -> Result<Vec<(String, String)>, StateError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let rows: Vec<(String, String)> = {
            let mut stmt = tx.prepare(
                "SELECT did, rev FROM repo
                  WHERE state = ?1 AND cooldown_until <= ?2
                  ORDER BY cooldown_until, did
                  LIMIT ?3",
            )?;
            let iter = stmt.query_map(
                params![
                    RepoState::Pending.as_str(),
                    now(),
                    i64::try_from(limit).unwrap_or(i64::MAX)
                ],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            iter.collect::<Result<_, _>>()?
        };
        for (did, _) in &rows {
            tx.execute(
                "UPDATE repo SET state = ?2, updated_at = ?3 WHERE did = ?1",
                params![did, RepoState::Claimed.as_str(), now()],
            )?;
        }
        tx.commit()?;
        drop(conn);
        Ok(rows)
    }

    /// Mark a repo indexed at `rev`.
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
    /// `transient` decides the shape: a transient failure goes back to pending
    /// with a cooldown until the attempt budget runs out; a terminal one is
    /// written off immediately rather than burning four more requests on a
    /// response that will not change.
    pub fn fail(
        &self,
        did: &str,
        transient: bool,
        cooldown_secs: u64,
        err: &str,
    ) -> Result<RepoState, StateError> {
        let conn = self.lock()?;
        let attempts: u32 = conn
            .query_row(
                "SELECT attempts FROM repo WHERE did = ?1",
                params![did],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            .try_into()
            .unwrap_or(0);
        let attempt_count = attempts.saturating_add(1);

        let next = if transient && attempt_count < MAX_ATTEMPTS {
            RepoState::Pending
        } else {
            RepoState::Terminal
        };
        let cooldown = if next == RepoState::Pending {
            now().saturating_add(cooldown_secs.try_into().unwrap_or(i64::MAX))
        } else {
            0
        };

        conn.execute(
            "UPDATE repo SET state = ?2, attempts = ?3, cooldown_until = ?4,
                             last_error = ?5, updated_at = ?6
              WHERE did = ?1",
            params![
                did,
                next.as_str(),
                i64::from(attempt_count),
                cooldown,
                err,
                now()
            ],
        )?;
        drop(conn);
        Ok(next)
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

    #[test]
    fn a_new_repo_needs_fetching() {
        let s = store();
        assert_eq!(
            s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap(),
            Upsert::NeedsFetch
        );
        assert_eq!(s.stats().unwrap().pending, 1);
    }

    #[test]
    fn re_enumeration_of_an_indexed_repo_is_a_no_op() {
        // The property the current producer cannot express: seeing the same
        // repo again must not create work.
        let s = store();
        s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap();
        let claimed = s.claim(10).unwrap();
        assert_eq!(claimed, vec![("did:a".to_owned(), "r1".to_owned())]);
        s.complete("did:a", "r1").unwrap();

        for _ in 0..5 {
            assert_eq!(
                s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap(),
                Upsert::Unchanged
            );
        }
        let st = s.stats().unwrap();
        assert_eq!((st.done, st.pending, st.total()), (1, 0, 1));
    }

    #[test]
    fn an_advanced_rev_makes_it_fetchable_again() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap();
        s.claim(10).unwrap();
        s.complete("did:a", "r1").unwrap();
        assert_eq!(
            s.upsert(&repo("did:a", "r2"), "hubble", false).unwrap(),
            Upsert::NeedsFetch
        );
        assert_eq!(s.stats().unwrap().pending, 1);
    }

    #[test]
    fn inactive_repos_are_written_off_without_a_fetch() {
        let s = store();
        assert_eq!(
            s.upsert(&repo("did:gone", "r1"), "hubble", true).unwrap(),
            Upsert::Skipped
        );
        assert!(s.claim(10).unwrap().is_empty());
        assert_eq!(s.stats().unwrap().terminal, 1);
    }

    #[test]
    fn a_reactivated_account_becomes_fetchable_again() {
        // Written off for being inactive, then upstream reports it active
        // again at the same rev. Waiting for the rev to move would strand it:
        // a reactivated account usually has not gained records.
        let s = store();
        assert_eq!(
            s.upsert(&repo("did:back", "r1"), "hubble", true).unwrap(),
            Upsert::Skipped
        );
        assert_eq!(s.stats().unwrap().terminal, 1);
        assert_eq!(
            s.upsert(&repo("did:back", "r1"), "hubble", false).unwrap(),
            Upsert::NeedsFetch
        );
        assert_eq!(s.claim(10).unwrap().len(), 1);
    }

    #[test]
    fn a_repo_written_off_for_failures_stays_written_off_at_the_same_rev() {
        let s = store();
        s.upsert(&repo("did:bad", "r1"), "hubble", false).unwrap();
        s.claim(10).unwrap();
        s.fail("did:bad", false, 0, "404").unwrap();
        assert_eq!(
            s.upsert(&repo("did:bad", "r1"), "hubble", false).unwrap(),
            Upsert::Skipped,
            "same rev, already failed: retrying would fail the same way"
        );
        assert_eq!(
            s.upsert(&repo("did:bad", "r2"), "hubble", false).unwrap(),
            Upsert::NeedsFetch,
            "a moved rev is genuinely new"
        );
    }

    #[test]
    fn an_empty_rev_never_counts_as_unchanged() {
        // Some relays omit rev. Without one there is nothing to compare, so the
        // safe direction is to re-fetch rather than skip.
        let s = store();
        s.upsert(&repo("did:a", ""), "relay", false).unwrap();
        let claimed = s.claim(10).unwrap();
        s.complete(&claimed[0].0, &claimed[0].1).unwrap();
        assert_eq!(
            s.upsert(&repo("did:a", ""), "relay", false).unwrap(),
            Upsert::NeedsFetch
        );
    }

    #[test]
    fn transient_failures_retry_with_cooldown_then_give_up() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap();
        for attempt in 1..MAX_ATTEMPTS {
            s.claim(10).unwrap();
            let next = s.fail("did:a", true, 0, "503").unwrap();
            assert_eq!(next, RepoState::Pending, "attempt {attempt} should retry");
        }
        s.claim(10).unwrap();
        assert_eq!(
            s.fail("did:a", true, 0, "503").unwrap(),
            RepoState::Terminal
        );
        assert_eq!(s.stats().unwrap().terminal, 1);
    }

    #[test]
    fn a_terminal_failure_does_not_spend_the_retry_budget() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap();
        s.claim(10).unwrap();
        assert_eq!(
            s.fail("did:a", false, 0, "404").unwrap(),
            RepoState::Terminal,
            "one 404 is enough"
        );
    }

    #[test]
    fn a_cooldown_hides_the_row_until_it_expires() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap();
        s.claim(10).unwrap();
        s.fail("did:a", true, 3600, "429").unwrap();
        assert!(s.claim(10).unwrap().is_empty(), "still cooling down");
        // Same row, no cooldown: immediately claimable.
        s.fail("did:a", true, 0, "429").unwrap();
        assert_eq!(s.claim(10).unwrap().len(), 1);
    }

    #[test]
    fn claimed_rows_are_recovered_after_a_crash() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap();
        s.upsert(&repo("did:b", "r1"), "hubble", false).unwrap();
        assert_eq!(s.claim(10).unwrap().len(), 2);
        assert_eq!(s.stats().unwrap().claimed, 2);
        assert_eq!(s.reset_claimed().unwrap(), 2);
        assert_eq!(s.stats().unwrap().pending, 2);
    }

    #[test]
    fn claim_is_exclusive() {
        let s = store();
        s.upsert(&repo("did:a", "r1"), "hubble", false).unwrap();
        assert_eq!(s.claim(10).unwrap().len(), 1);
        assert!(s.claim(10).unwrap().is_empty(), "already claimed");
    }

    #[test]
    fn a_batch_reports_what_it_did() {
        let s = store();
        let page = vec![
            repo("did:a", "r1"),
            repo("did:b", "r1"),
            repo("did:c", "r1"),
        ];
        let (fetch, unchanged, skipped) = s
            .upsert_batch(&page, "hubble", &|r| r.did == "did:c")
            .unwrap();
        assert_eq!((fetch, unchanged, skipped), (2, 0, 1));

        s.claim(10).unwrap();
        s.complete("did:a", "r1").unwrap();
        let (fetch, unchanged, skipped) = s
            .upsert_batch(&page, "hubble", &|r| r.did == "did:c")
            .unwrap();
        assert_eq!(
            (fetch, unchanged, skipped),
            (1, 1, 1),
            "a completed repo is unchanged on the next pass"
        );
    }

    #[test]
    fn cursors_round_trip_as_text() {
        // The regression that matters: a DID cursor must survive, where the
        // existing i64 column turns it into 0.
        let s = store();
        assert_eq!(s.get_cursor("hubble").unwrap(), None);
        s.set_cursor("hubble", "did:plc:222rpxpbdd4cd2opywsqpxtz")
            .unwrap();
        assert_eq!(
            s.get_cursor("hubble").unwrap().as_deref(),
            Some("did:plc:222rpxpbdd4cd2opywsqpxtz")
        );
        s.set_cursor("hubble", "did:plc:zzz").unwrap();
        assert_eq!(
            s.get_cursor("hubble").unwrap().as_deref(),
            Some("did:plc:zzz")
        );
        s.clear_cursor("hubble").unwrap();
        assert_eq!(s.get_cursor("hubble").unwrap(), None);
    }

    #[test]
    fn an_integer_relay_cursor_also_round_trips() {
        let s = store();
        s.set_cursor("relay", "1234567").unwrap();
        assert_eq!(s.get_cursor("relay").unwrap().as_deref(), Some("1234567"));
    }
}
