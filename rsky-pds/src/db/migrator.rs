// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/db/migrator.ts

use crate::db::sqlite::Db;
use anyhow::{bail, Result};
use rusqlite::Transaction;
use std::collections::HashSet;

/// A named, embedded SQL migration. Migrations are applied in slice order
/// and tracked by name in a `migrations` table.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub name: &'static str,
    pub sql: &'static str,
}

const CREATE_MIGRATIONS_TABLE: &str = "CREATE TABLE IF NOT EXISTS migrations (\
     name TEXT PRIMARY KEY, \
     \"appliedAt\" TEXT NOT NULL\
     )";

fn applied_migrations(tx: &Transaction) -> Result<HashSet<String>> {
    let mut stmt = tx.prepare("SELECT name FROM migrations")?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<String>, rusqlite::Error>>()?;
    Ok(names)
}

/// Applies any unapplied migrations, in order, within a single transaction.
pub async fn migrate_to_latest(db: &Db, migrations: &'static [Migration]) -> Result<()> {
    db.tx(move |tx| {
        tx.execute_batch(CREATE_MIGRATIONS_TABLE)?;
        let applied = applied_migrations(tx)?;
        let known: HashSet<&str> = migrations.iter().map(|m| m.name).collect();
        for name in &applied {
            if !known.contains(name.as_str()) {
                bail!("unknown migration previously applied: {name}");
            }
        }
        let mut seen_unapplied = false;
        for migration in migrations {
            if applied.contains(migration.name) {
                if seen_unapplied {
                    bail!("migrations applied out of order at: {}", migration.name);
                }
                continue;
            }
            seen_unapplied = true;
            tx.execute_batch(migration.sql)?;
            tx.execute(
                "INSERT INTO migrations (name, \"appliedAt\") VALUES (?1, ?2)",
                rusqlite::params![migration.name, rsky_common::now()],
            )?;
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIGRATIONS_ONE: &[Migration] = &[Migration {
        name: "001",
        sql: "CREATE TABLE first (id INTEGER PRIMARY KEY)",
    }];

    const MIGRATIONS_TWO: &[Migration] = &[
        Migration {
            name: "001",
            sql: "CREATE TABLE first (id INTEGER PRIMARY KEY)",
        },
        Migration {
            name: "002",
            sql: "CREATE TABLE second (id INTEGER PRIMARY KEY)",
        },
    ];

    const MIGRATIONS_SECOND_ONLY: &[Migration] = &[Migration {
        name: "002",
        sql: "CREATE TABLE second (id INTEGER PRIMARY KEY)",
    }];

    const MIGRATIONS_BROKEN: &[Migration] = &[Migration {
        name: "001",
        sql: "CREATE TABLE first (id INTEGER PRIMARY KEY); NOT VALID SQL",
    }];

    fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("test.sqlite")).unwrap();
        (dir, db)
    }

    async fn table_names(db: &Db) -> Vec<String> {
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

    #[tokio::test]
    async fn applies_migrations_in_order() {
        let (_dir, db) = temp_db();
        migrate_to_latest(&db, MIGRATIONS_TWO).await.unwrap();
        assert_eq!(table_names(&db).await, ["first", "migrations", "second"]);
    }

    #[tokio::test]
    async fn is_idempotent_and_applies_new_migrations() {
        let (_dir, db) = temp_db();
        migrate_to_latest(&db, MIGRATIONS_ONE).await.unwrap();
        migrate_to_latest(&db, MIGRATIONS_ONE).await.unwrap();
        migrate_to_latest(&db, MIGRATIONS_TWO).await.unwrap();
        migrate_to_latest(&db, MIGRATIONS_TWO).await.unwrap();
        assert_eq!(table_names(&db).await, ["first", "migrations", "second"]);
        let applied: i64 = db
            .run(
                |conn| Ok(conn.query_row("SELECT count(*) FROM migrations", [], |row| row.get(0))?),
            )
            .await
            .unwrap();
        assert_eq!(applied, 2);
    }

    #[tokio::test]
    async fn rejects_unknown_applied_migration() {
        let (_dir, db) = temp_db();
        migrate_to_latest(&db, MIGRATIONS_TWO).await.unwrap();
        let err = migrate_to_latest(&db, MIGRATIONS_ONE).await.unwrap_err();
        assert!(err.to_string().contains("unknown migration"));
    }

    #[tokio::test]
    async fn rejects_out_of_order_migrations() {
        let (_dir, db) = temp_db();
        migrate_to_latest(&db, MIGRATIONS_SECOND_ONLY)
            .await
            .unwrap();
        let err = migrate_to_latest(&db, MIGRATIONS_TWO).await.unwrap_err();
        assert!(err.to_string().contains("out of order"));
    }

    #[tokio::test]
    async fn rolls_back_failed_migration() {
        let (_dir, db) = temp_db();
        let res = migrate_to_latest(&db, MIGRATIONS_BROKEN).await;
        assert!(res.is_err());
        assert!(table_names(&db).await.is_empty());
        // a fixed set applies cleanly afterwards
        migrate_to_latest(&db, MIGRATIONS_ONE).await.unwrap();
        assert_eq!(table_names(&db).await, ["first", "migrations"]);
    }
}
