// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/db/db.ts
// and https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/db/util.ts

use anyhow::Result;
use rusqlite::{Connection, ErrorCode, Transaction, TransactionBehavior};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// based on sqlite's backoff strategy
// https://github.com/sqlite/sqlite/blob/91c8e65dd4bf17d21fbf8f7073565fe1a71c8948/src/main.c#L1704-L1713
const DELAYS_MS: [u64; 12] = [1, 2, 5, 10, 15, 20, 25, 25, 25, 50, 50, 100];
const TOTALS_MS: [u64; 12] = [0, 1, 3, 8, 18, 33, 53, 78, 103, 128, 178, 228];
const RETRY_TIMEOUT_MS: u64 = 5000;

pub(crate) fn retry_wait_ms(n: usize, timeout_ms: u64) -> Option<u64> {
    let last_idx = DELAYS_MS.len() - 1;
    let (mut delay, prior) = if n < DELAYS_MS.len() {
        (DELAYS_MS[n], TOTALS_MS[n])
    } else {
        let delay = DELAYS_MS[last_idx];
        (delay, TOTALS_MS[last_idx] + delay * (n - last_idx) as u64)
    };
    if prior + delay > timeout_ms {
        if timeout_ms <= prior {
            return None;
        }
        delay = timeout_ms - prior;
    }
    Some(delay)
}

pub(crate) fn is_busy_error(err: &anyhow::Error) -> bool {
    match err.downcast_ref::<rusqlite::Error>() {
        Some(sqlite_err) => matches!(
            sqlite_err.sqlite_error_code(),
            Some(ErrorCode::DatabaseBusy)
        ),
        None => false,
    }
}

fn retry_sqlite<T>(mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let mut attempt: usize = 0;
    loop {
        match f() {
            Ok(res) => return Ok(res),
            Err(err) if is_busy_error(&err) => match retry_wait_ms(attempt, RETRY_TIMEOUT_MS) {
                Some(wait_ms) => {
                    crate::metrics::METRICS.sqlite_busy_retries.inc();
                    std::thread::sleep(Duration::from_millis(wait_ms));
                    attempt += 1;
                }
                None => return Err(err),
            },
            Err(err) => return Err(err),
        }
    }
}

/// How much of a write sqlite waits for before reporting it committed.
///
/// `Normal` is durable across process crashes but a power loss or kernel
/// panic can lose the last transactions; `Full` waits for the WAL to reach
/// stable storage on every commit, which a writer whose commits are
/// acknowledged to clients needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Synchronous {
    Normal,
    Full,
}

impl Synchronous {
    fn pragma_value(self) -> &'static str {
        match self {
            Synchronous::Normal => "NORMAL",
            Synchronous::Full => "FULL",
        }
    }
}

/// An async wrapper over a single rusqlite connection.
/// All access happens on the blocking threadpool while holding the
/// connection mutex, so statements never interleave mid-execution.
#[derive(Clone, Debug)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(location: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(location, Synchronous::Normal)
    }

    pub fn open_with(location: impl AsRef<Path>, synchronous: Synchronous) -> Result<Self> {
        let conn = Connection::open(location)?;
        Self::setup_conn(&conn, synchronous)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Opens an existing database so that no statement can change it; the
    /// file's journal mode is left as it is.
    pub fn open_read_only(location: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open_with_flags(
            location,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_millis(RETRY_TIMEOUT_MS))?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn setup_conn(conn: &Connection, synchronous: Synchronous) -> Result<()> {
        let journal_mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            tracing::warn!(%journal_mode, "sqlite db not using WAL journal mode");
        }
        conn.pragma_update(None, "synchronous", synchronous.pragma_value())?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(Duration::from_millis(RETRY_TIMEOUT_MS))?;
        Ok(())
    }

    /// The file this connection is open on, when it is a file.
    pub fn path(&self) -> Option<std::path::PathBuf> {
        let conn = self.conn.lock().expect("sqlite connection mutex poisoned");
        conn.path().map(std::path::PathBuf::from)
    }

    /// The `synchronous` level this connection was opened with.
    pub fn synchronous(&self) -> Result<Synchronous> {
        let conn = self.conn.lock().expect("sqlite connection mutex poisoned");
        let level: i64 = conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
        Ok(if level >= 2 {
            Synchronous::Full
        } else {
            Synchronous::Normal
        })
    }

    /// Runs `f` against the connection on the blocking threadpool,
    /// retrying with backoff while sqlite reports busy.
    pub async fn run<F, T>(&self, f: F) -> Result<T>
    where
        F: FnMut(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut f = f;
            let mut conn = conn.lock().expect("sqlite connection mutex poisoned");
            retry_sqlite(|| f(&mut conn))
        })
        .await?
    }

    /// Runs `f` inside a `BEGIN IMMEDIATE` transaction, committing on Ok
    /// and rolling back on Err. The entire transaction is retried while
    /// sqlite reports busy.
    pub async fn tx<F, T>(&self, f: F) -> Result<T>
    where
        F: FnMut(&Transaction) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut f = f;
            let mut conn = conn.lock().expect("sqlite connection mutex poisoned");
            retry_sqlite(|| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let res = f(&tx)?;
                tx.commit()?;
                Ok(res)
            })
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("test.sqlite")).unwrap();
        (dir, db)
    }

    #[tokio::test]
    async fn open_applies_pragmas() {
        let (_dir, db) = temp_db();
        let (journal_mode, synchronous, foreign_keys) = db
            .run(|conn| {
                let journal_mode: String =
                    conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
                let synchronous: i64 =
                    conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
                let foreign_keys: i64 =
                    conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
                Ok((journal_mode, synchronous, foreign_keys))
            })
            .await
            .unwrap();
        assert_eq!(journal_mode, "wal");
        assert_eq!(synchronous, 1);
        assert_eq!(foreign_keys, 1);
    }

    #[tokio::test]
    async fn open_with_full_synchronous() {
        let dir = tempfile::tempdir().unwrap();
        let location = dir.path().join("full.sqlite");
        let db = Db::open_with(&location, Synchronous::Full).unwrap();
        assert_eq!(db.synchronous().unwrap(), Synchronous::Full);
        assert_eq!(
            db.path().unwrap().canonicalize().unwrap(),
            location.canonicalize().unwrap()
        );
        let normal = Db::open(dir.path().join("normal.sqlite")).unwrap();
        assert_eq!(normal.synchronous().unwrap(), Synchronous::Normal);
    }

    #[tokio::test]
    async fn read_only_connections_refuse_every_write() {
        let dir = tempfile::tempdir().unwrap();
        let location = dir.path().join("ro.sqlite");
        Db::open(&location)
            .unwrap()
            .run(|conn| {
                conn.execute_batch(
                    "CREATE TABLE t (val TEXT NOT NULL); INSERT INTO t VALUES ('a')",
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let db = Db::open_read_only(&location).unwrap();
        let val: String = db
            .run(|conn| Ok(conn.query_row("SELECT val FROM t", [], |row| row.get(0))?))
            .await
            .unwrap();
        assert_eq!(val, "a");
        let err = db
            .run(|conn| {
                conn.execute("INSERT INTO t VALUES ('b')", [])?;
                Ok(())
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("readonly"), "{err}");
        assert!(Db::open_read_only(dir.path().join("missing.sqlite")).is_err());
    }

    #[tokio::test]
    async fn warns_when_wal_unavailable() {
        // in-memory databases cannot use WAL; open still succeeds
        let db = Db::open(":memory:").unwrap();
        let journal_mode: String = db
            .run(|conn| Ok(conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?))
            .await
            .unwrap();
        assert_eq!(journal_mode, "memory");
    }

    #[tokio::test]
    async fn run_round_trips_data() {
        let (_dir, db) = temp_db();
        db.run(|conn| {
            conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT NOT NULL)")?;
            conn.execute("INSERT INTO t (val) VALUES (?1)", ["hello"])?;
            Ok(())
        })
        .await
        .unwrap();
        let val: String = db
            .run(|conn| Ok(conn.query_row("SELECT val FROM t", [], |row| row.get(0))?))
            .await
            .unwrap();
        assert_eq!(val, "hello");
    }

    #[tokio::test]
    async fn run_propagates_errors() {
        let (_dir, db) = temp_db();
        let err = db
            .run(|conn| {
                conn.execute("SELECT * FROM missing_table", [])?;
                Ok(())
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("missing_table"));
    }

    #[tokio::test]
    async fn tx_commits_on_ok() {
        let (_dir, db) = temp_db();
        db.tx(|tx| {
            tx.execute_batch("CREATE TABLE t (val TEXT NOT NULL)")?;
            tx.execute("INSERT INTO t (val) VALUES ('a')", [])?;
            tx.execute("INSERT INTO t (val) VALUES ('b')", [])?;
            Ok(())
        })
        .await
        .unwrap();
        let count: i64 = db
            .run(|conn| Ok(conn.query_row("SELECT count(*) FROM t", [], |row| row.get(0))?))
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn tx_rolls_back_on_err() {
        let (_dir, db) = temp_db();
        db.run(|conn| {
            conn.execute_batch("CREATE TABLE t (val TEXT NOT NULL)")?;
            Ok(())
        })
        .await
        .unwrap();
        let res: Result<()> = db
            .tx(|tx| {
                tx.execute("INSERT INTO t (val) VALUES ('a')", [])?;
                anyhow::bail!("boom")
            })
            .await;
        assert!(res.is_err());
        let count: i64 = db
            .run(|conn| Ok(conn.query_row("SELECT count(*) FROM t", [], |row| row.get(0))?))
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn retries_on_busy_then_succeeds() {
        let (_dir, db) = temp_db();
        let mut attempts = 0;
        let res = db
            .run(move |_conn| {
                attempts += 1;
                if attempts < 3 {
                    Err(anyhow::Error::new(rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                        Some("database is locked".to_owned()),
                    )))
                } else {
                    Ok(attempts)
                }
            })
            .await
            .unwrap();
        assert_eq!(res, 3);
    }

    #[test]
    fn busy_error_detection() {
        let busy = anyhow::Error::new(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        ));
        assert!(is_busy_error(&busy));
        let other_sqlite = anyhow::Error::new(rusqlite::Error::QueryReturnedNoRows);
        assert!(!is_busy_error(&other_sqlite));
        let not_sqlite = anyhow::anyhow!("nope");
        assert!(!is_busy_error(&not_sqlite));
    }

    #[test]
    fn retry_wait_follows_sqlite_backoff() {
        assert_eq!(retry_wait_ms(0, RETRY_TIMEOUT_MS), Some(1));
        assert_eq!(retry_wait_ms(5, RETRY_TIMEOUT_MS), Some(20));
        assert_eq!(retry_wait_ms(11, RETRY_TIMEOUT_MS), Some(100));
        // beyond the table it keeps the max delay
        assert_eq!(retry_wait_ms(12, RETRY_TIMEOUT_MS), Some(100));
        // truncates the delay as it approaches the timeout
        assert_eq!(retry_wait_ms(11, 300), Some(300 - 228));
        // and gives up entirely once the timeout is spent
        assert_eq!(retry_wait_ms(12, 300), None);
        assert_eq!(retry_wait_ms(11, 228), None);
        assert_eq!(retry_wait_ms(60, RETRY_TIMEOUT_MS), None);
    }

    #[tokio::test]
    async fn gives_up_after_timeout_of_busy() {
        let (_dir, db) = temp_db();
        // hold a write lock on a second connection to the same file
        let path = db
            .run(|conn| Ok(conn.path().unwrap().to_owned()))
            .await
            .unwrap();
        let blocker = Connection::open(&path).unwrap();
        blocker.busy_timeout(Duration::from_millis(0)).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();

        let res = db
            .run(move |conn| {
                conn.busy_timeout(Duration::from_millis(0))?;
                conn.execute_batch("BEGIN IMMEDIATE")?;
                conn.execute_batch("COMMIT")?;
                Ok(())
            })
            .await;
        assert!(res.is_err());
        assert!(is_busy_error(&res.unwrap_err()));
    }
}
