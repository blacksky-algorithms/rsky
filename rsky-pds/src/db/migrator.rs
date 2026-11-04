// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/db/migrator.ts

use crate::db::sqlite::Db;
use anyhow::{bail, Result};
use rusqlite::{params, Transaction};
use std::collections::HashSet;

/// A named, embedded SQL migration. Migrations are applied in slice order.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub name: &'static str,
    pub sql: &'static str,
}

/// Migrations for one database, split across two ledgers.
///
/// `shared` migrations reproduce the reference PDS schema and are recorded in
/// Kysely's `kysely_migration` ledger under the reference migration names, so
/// a database can be opened by either implementation. `local` migrations are
/// rsky-only additions recorded in the `migrations` ledger, which the
/// reference PDS never reads. `legacy` converts a database created before the
/// ledgers were split; it runs once, before any migration is applied.
#[derive(Debug, Clone, Copy)]
pub struct MigrationSet {
    pub shared: &'static [Migration],
    pub local: &'static [Migration],
    pub legacy: Option<fn(&Transaction) -> Result<()>>,
}

/// Which migrations of a set a database still lacks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pending {
    pub shared: Vec<&'static str>,
    pub local: Vec<&'static str>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.shared.is_empty() && self.local.is_empty()
    }
}

const SHARED_LEDGER: &str = "kysely_migration";
const LOCAL_LEDGER: &str = "migrations";

const CREATE_SHARED_LEDGER: &str = "\
    CREATE TABLE IF NOT EXISTS kysely_migration (\
        name varchar(255) NOT NULL PRIMARY KEY, \
        timestamp varchar(255) NOT NULL\
    );\
    CREATE TABLE IF NOT EXISTS kysely_migration_lock (\
        id varchar(255) NOT NULL PRIMARY KEY, \
        is_locked integer NOT NULL DEFAULT 0\
    );\
    INSERT OR IGNORE INTO kysely_migration_lock (id, is_locked) VALUES ('migration_lock', 0);";

const CREATE_LOCAL_LEDGER: &str = "\
    CREATE TABLE IF NOT EXISTS migrations (\
        name TEXT PRIMARY KEY, \
        \"appliedAt\" TEXT NOT NULL\
    )";

pub(crate) fn table_exists(tx: &Transaction, name: &str) -> Result<bool> {
    let count: i64 = tx.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn ledger_names(tx: &Transaction, ledger: &str) -> Result<HashSet<String>> {
    if !table_exists(tx, ledger)? {
        return Ok(HashSet::new());
    }
    let mut stmt = tx.prepare(&format!("SELECT name FROM {ledger}"))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<String>, rusqlite::Error>>()?;
    Ok(names)
}

/// Databases created before the ledgers were split recorded shared
/// migrations in the local ledger. Move them across so both ledgers describe
/// the database the way a freshly migrated one would.
fn convert_legacy(tx: &Transaction, set: &MigrationSet) -> Result<()> {
    let shared_names: HashSet<&str> = set.shared.iter().map(|m| m.name).collect();
    let misfiled = |tx: &Transaction| -> Result<Vec<String>> {
        let mut names: Vec<String> = ledger_names(tx, LOCAL_LEDGER)?
            .into_iter()
            .filter(|name| shared_names.contains(name.as_str()))
            .collect();
        names.sort();
        Ok(names)
    };
    if misfiled(tx)?.is_empty() {
        return Ok(());
    }
    if let Some(legacy) = set.legacy {
        legacy(tx)?;
    }
    tx.execute_batch(CREATE_SHARED_LEDGER)?;
    for name in misfiled(tx)? {
        tx.execute(
            "INSERT OR IGNORE INTO kysely_migration (name, timestamp) VALUES (?1, ?2)",
            params![name, rsky_common::now()],
        )?;
        tx.execute("DELETE FROM migrations WHERE name = ?1", [name])?;
    }
    Ok(())
}

fn missing<'a>(
    applied: &HashSet<String>,
    migrations: &'a [Migration],
    ledger: &str,
) -> Result<Vec<&'a Migration>> {
    let mut seen_unapplied = false;
    let mut pending = Vec::new();
    for migration in migrations {
        if applied.contains(migration.name) {
            if seen_unapplied {
                bail!(
                    "{ledger} migrations applied out of order at: {}",
                    migration.name
                );
            }
            continue;
        }
        seen_unapplied = true;
        pending.push(migration);
    }
    Ok(pending)
}

fn reject_unknown(applied: &HashSet<String>, known: &HashSet<&str>, ledger: &str) -> Result<()> {
    for name in applied {
        if !known.contains(name.as_str()) {
            bail!("unknown {ledger} migration previously applied: {name}");
        }
    }
    Ok(())
}

fn inspect(tx: &Transaction, set: &MigrationSet) -> Result<Pending> {
    let shared_known: HashSet<&str> = set.shared.iter().map(|m| m.name).collect();
    // an unconverted legacy ledger may still hold shared names; they are
    // reported as pending rather than unknown
    let local_known: HashSet<&str> = set
        .local
        .iter()
        .map(|m| m.name)
        .chain(shared_known.iter().copied())
        .collect();
    let shared_applied = ledger_names(tx, SHARED_LEDGER)?;
    let local_applied = ledger_names(tx, LOCAL_LEDGER)?;
    reject_unknown(&shared_applied, &shared_known, SHARED_LEDGER)?;
    reject_unknown(&local_applied, &local_known, LOCAL_LEDGER)?;
    Ok(Pending {
        shared: missing(&shared_applied, set.shared, SHARED_LEDGER)?
            .into_iter()
            .map(|m| m.name)
            .collect(),
        local: missing(&local_applied, set.local, LOCAL_LEDGER)?
            .into_iter()
            .map(|m| m.name)
            .collect(),
    })
}

/// Reports which migrations a database lacks without applying any. Legacy
/// ledger rows are moved across, which is the only write this performs.
pub async fn pending(db: &Db, set: MigrationSet) -> Result<Pending> {
    db.tx(move |tx| {
        convert_legacy(tx, &set)?;
        inspect(tx, &set)
    })
    .await
}

/// Reports which migrations a database lacks, touching nothing. Legacy
/// ledger rows are reported as missing rather than converted.
pub async fn pending_read_only(db: &Db, set: MigrationSet) -> Result<Pending> {
    db.run(move |conn| {
        let tx = conn.transaction()?;
        let pending = inspect(&tx, &set)?;
        tx.rollback()?;
        Ok(pending)
    })
    .await
}

/// Applies any unapplied migrations of both ledgers, in order, within a
/// single transaction.
pub async fn migrate_to_latest(db: &Db, set: MigrationSet) -> Result<()> {
    db.tx(move |tx| {
        convert_legacy(tx, &set)?;
        let pending = inspect(tx, &set)?;
        if pending.is_empty() {
            return Ok(());
        }
        tx.execute_batch(CREATE_SHARED_LEDGER)?;
        if !pending.local.is_empty() {
            tx.execute_batch(CREATE_LOCAL_LEDGER)?;
        }
        for migration in set
            .shared
            .iter()
            .filter(|m| pending.shared.contains(&m.name))
        {
            tx.execute_batch(migration.sql)?;
            tx.execute(
                "INSERT INTO kysely_migration (name, timestamp) VALUES (?1, ?2)",
                params![migration.name, rsky_common::now()],
            )?;
        }
        for migration in set.local.iter().filter(|m| pending.local.contains(&m.name)) {
            tx.execute_batch(migration.sql)?;
            tx.execute(
                "INSERT INTO migrations (name, \"appliedAt\") VALUES (?1, ?2)",
                params![migration.name, rsky_common::now()],
            )?;
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: Migration = Migration {
        name: "001",
        sql: "CREATE TABLE first (id INTEGER PRIMARY KEY)",
    };
    const SECOND: Migration = Migration {
        name: "002",
        sql: "CREATE TABLE second (id INTEGER PRIMARY KEY)",
    };
    const EXTRA: Migration = Migration {
        name: "002",
        sql: "CREATE TABLE extra (id INTEGER PRIMARY KEY)",
    };
    const BROKEN: Migration = Migration {
        name: "001",
        sql: "CREATE TABLE first (id INTEGER PRIMARY KEY); NOT VALID SQL",
    };

    const ONE: MigrationSet = MigrationSet {
        shared: &[FIRST],
        local: &[],
        legacy: None,
    };
    const TWO: MigrationSet = MigrationSet {
        shared: &[FIRST, SECOND],
        local: &[],
        legacy: None,
    };
    const SECOND_ONLY: MigrationSet = MigrationSet {
        shared: &[SECOND],
        local: &[],
        legacy: None,
    };
    const BROKEN_SET: MigrationSet = MigrationSet {
        shared: &[BROKEN],
        local: &[],
        legacy: None,
    };
    const WITH_LOCAL: MigrationSet = MigrationSet {
        shared: &[FIRST],
        local: &[EXTRA],
        legacy: None,
    };

    fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("test.sqlite")).unwrap();
        (dir, db)
    }

    pub(crate) async fn table_names(db: &Db) -> Vec<String> {
        db.run(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' \
                 AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )?;
            let names = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<String>, rusqlite::Error>>()?;
            Ok(names)
        })
        .await
        .unwrap()
    }

    pub(crate) async fn ledger(db: &Db, ledger: &'static str) -> Vec<String> {
        db.run(move |conn| {
            let mut stmt = conn.prepare(&format!("SELECT name FROM {ledger} ORDER BY name"))?;
            let names = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<String>, rusqlite::Error>>()?;
            Ok(names)
        })
        .await
        .unwrap()
    }

    /// Mimics a database the reference PDS created: Kysely's ledger holds the
    /// shared names and no local ledger exists.
    pub(crate) async fn reference_db(db: &Db, applied: &'static [Migration]) {
        db.run(move |conn| {
            conn.execute_batch(CREATE_SHARED_LEDGER)?;
            for m in applied {
                conn.execute_batch(m.sql)?;
                conn.execute(
                    "INSERT INTO kysely_migration (name, timestamp) VALUES (?1, ?2)",
                    params![m.name, "2026-01-01T00:00:00.000Z"],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();
    }

    /// Mimics a database an earlier rsky release created: everything sits in
    /// the local ledger.
    pub(crate) async fn legacy_db(db: &Db, applied: &'static [Migration]) {
        db.run(move |conn| {
            conn.execute_batch(CREATE_LOCAL_LEDGER)?;
            for m in applied {
                conn.execute_batch(m.sql)?;
                conn.execute(
                    "INSERT INTO migrations (name, \"appliedAt\") VALUES (?1, ?2)",
                    params![m.name, "2026-01-01T00:00:00.000Z"],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn applies_shared_migrations_into_the_reference_ledger() {
        let (_dir, db) = temp_db();
        migrate_to_latest(&db, TWO).await.unwrap();
        assert_eq!(
            table_names(&db).await,
            [
                "first",
                "kysely_migration",
                "kysely_migration_lock",
                "second"
            ]
        );
        assert_eq!(ledger(&db, SHARED_LEDGER).await, ["001", "002"]);
        let locked: i64 = db
            .run(|conn| {
                Ok(conn.query_row(
                    "SELECT is_locked FROM kysely_migration_lock WHERE id = 'migration_lock'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(locked, 0);
    }

    #[tokio::test]
    async fn applies_local_migrations_into_the_local_ledger() {
        let (_dir, db) = temp_db();
        migrate_to_latest(&db, WITH_LOCAL).await.unwrap();
        assert_eq!(ledger(&db, SHARED_LEDGER).await, ["001"]);
        assert_eq!(ledger(&db, LOCAL_LEDGER).await, ["002"]);
        assert!(table_names(&db).await.contains(&"extra".to_string()));
    }

    #[tokio::test]
    async fn is_idempotent_and_applies_new_migrations() {
        let (_dir, db) = temp_db();
        migrate_to_latest(&db, ONE).await.unwrap();
        migrate_to_latest(&db, ONE).await.unwrap();
        migrate_to_latest(&db, TWO).await.unwrap();
        migrate_to_latest(&db, TWO).await.unwrap();
        assert_eq!(ledger(&db, SHARED_LEDGER).await, ["001", "002"]);
    }

    #[tokio::test]
    async fn opens_a_reference_database_without_reapplying() {
        let (_dir, db) = temp_db();
        reference_db(&db, &[FIRST, SECOND]).await;
        assert_eq!(pending(&db, TWO).await.unwrap(), Pending::default());
        migrate_to_latest(&db, TWO).await.unwrap();
        assert_eq!(ledger(&db, SHARED_LEDGER).await, ["001", "002"]);
        assert!(!table_names(&db).await.contains(&"migrations".to_string()));
    }

    #[tokio::test]
    async fn pending_reports_local_migrations_a_reference_database_lacks() {
        let (_dir, db) = temp_db();
        reference_db(&db, &[FIRST]).await;
        let pending = pending(&db, WITH_LOCAL).await.unwrap();
        assert_eq!(pending.shared, Vec::<&str>::new());
        assert_eq!(pending.local, ["002"]);
        assert!(!pending.is_empty());
        assert!(!table_names(&db).await.contains(&"extra".to_string()));
    }

    #[tokio::test]
    async fn pending_read_only_touches_nothing() {
        let (_dir, db) = temp_db();
        legacy_db(&db, &[FIRST]).await;
        let pending = pending_read_only(&db, WITH_LOCAL).await.unwrap();
        // the legacy row is neither converted nor recognised as shared
        assert_eq!(pending.shared, ["001"]);
        assert_eq!(pending.local, ["002"]);
        assert!(!table_names(&db)
            .await
            .contains(&"kysely_migration".to_string()));
    }

    #[tokio::test]
    async fn converts_a_legacy_local_ledger() {
        let (_dir, db) = temp_db();
        legacy_db(&db, &[FIRST, EXTRA]).await;
        migrate_to_latest(&db, WITH_LOCAL).await.unwrap();
        assert_eq!(ledger(&db, SHARED_LEDGER).await, ["001"]);
        assert_eq!(ledger(&db, LOCAL_LEDGER).await, ["002"]);
        assert_eq!(pending(&db, WITH_LOCAL).await.unwrap(), Pending::default());
    }

    #[tokio::test]
    async fn legacy_conversion_runs_the_schema_hook_once() {
        fn hook(tx: &Transaction) -> Result<()> {
            tx.execute_batch("CREATE TABLE converted (id INTEGER PRIMARY KEY)")?;
            Ok(())
        }
        const HOOKED: MigrationSet = MigrationSet {
            shared: &[FIRST],
            local: &[],
            legacy: Some(hook),
        };
        let (_dir, db) = temp_db();
        legacy_db(&db, &[FIRST]).await;
        migrate_to_latest(&db, HOOKED).await.unwrap();
        migrate_to_latest(&db, HOOKED).await.unwrap();
        assert!(table_names(&db).await.contains(&"converted".to_string()));
        assert_eq!(ledger(&db, SHARED_LEDGER).await, ["001"]);
    }

    #[tokio::test]
    async fn rejects_unknown_shared_migration() {
        let (_dir, db) = temp_db();
        reference_db(&db, &[FIRST, SECOND]).await;
        let err = migrate_to_latest(&db, ONE).await.unwrap_err();
        assert!(err
            .to_string()
            .contains("unknown kysely_migration migration"));
    }

    #[tokio::test]
    async fn rejects_unknown_local_migration() {
        let (_dir, db) = temp_db();
        legacy_db(&db, &[FIRST, EXTRA]).await;
        // "002" is neither a shared nor a local name of this set
        let err = migrate_to_latest(&db, ONE).await.unwrap_err();
        assert!(err.to_string().contains("unknown migrations migration"));
    }

    #[tokio::test]
    async fn rejects_out_of_order_migrations() {
        let (_dir, db) = temp_db();
        migrate_to_latest(&db, SECOND_ONLY).await.unwrap();
        let err = migrate_to_latest(&db, TWO).await.unwrap_err();
        assert!(err.to_string().contains("out of order"));
    }

    #[tokio::test]
    async fn rolls_back_failed_migration() {
        let (_dir, db) = temp_db();
        let res = migrate_to_latest(&db, BROKEN_SET).await;
        assert!(res.is_err());
        assert!(table_names(&db).await.is_empty());
        migrate_to_latest(&db, ONE).await.unwrap();
        assert!(table_names(&db).await.contains(&"first".to_string()));
    }
}
