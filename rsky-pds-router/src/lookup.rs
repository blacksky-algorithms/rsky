//! Read-only lookups in the shared `account.sqlite`: the account behind a
//! handle, an email, or an email token, for attributing a request to the
//! account it mutates.

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

pub struct AccountLookup {
    conn: Mutex<Connection>,
}

impl AccountLookup {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(2000))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn one(&self, sql: &str, param: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .lock()
            .expect("lookup poisoned")
            .query_row(sql, [param], |row| row.get(0))
            .optional()
    }

    /// The DID an identifier names: a DID as itself, a handle through the
    /// actor table, an email through the account table.
    pub fn did_for_identifier(&self, identifier: &str) -> rusqlite::Result<Option<String>> {
        if identifier.starts_with("did:") {
            return Ok(Some(identifier.to_owned()));
        }
        let normalized = identifier.trim().to_ascii_lowercase();
        if normalized.contains('@') {
            return self.did_for_email(&normalized);
        }
        self.one("SELECT did FROM actor WHERE handle = ?1", &normalized)
    }

    pub fn did_for_email(&self, email: &str) -> rusqlite::Result<Option<String>> {
        self.one(
            "SELECT did FROM account WHERE email = ?1",
            &email.trim().to_ascii_lowercase(),
        )
    }

    pub fn did_for_email_token(&self, token: &str) -> rusqlite::Result<Option<String>> {
        self.one(
            "SELECT did FROM email_token WHERE token = ?1",
            &token.trim().to_ascii_uppercase(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_resolve_through_the_account_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("account.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE actor (did TEXT PRIMARY KEY, handle TEXT);
             CREATE TABLE account (did TEXT PRIMARY KEY, email TEXT);
             CREATE TABLE email_token (purpose TEXT, did TEXT, token TEXT);
             INSERT INTO actor VALUES ('did:plc:a', 'alice.test');
             INSERT INTO account VALUES ('did:plc:a', 'alice@example.com');
             INSERT INTO email_token VALUES ('reset_password', 'did:plc:a', 'ABCDE-FGHIJ');",
        )
        .unwrap();
        let lookup = AccountLookup::open(&path).unwrap();
        assert_eq!(
            lookup.did_for_identifier("did:plc:zzz").unwrap().as_deref(),
            Some("did:plc:zzz")
        );
        assert_eq!(
            lookup.did_for_identifier("Alice.Test ").unwrap().as_deref(),
            Some("did:plc:a")
        );
        assert_eq!(
            lookup
                .did_for_identifier("ALICE@example.com")
                .unwrap()
                .as_deref(),
            Some("did:plc:a")
        );
        assert!(lookup.did_for_identifier("nobody.test").unwrap().is_none());
        assert_eq!(
            lookup
                .did_for_email_token("abcde-fghij")
                .unwrap()
                .as_deref(),
            Some("did:plc:a")
        );
        assert!(lookup.did_for_email_token("nope").unwrap().is_none());
        assert!(AccountLookup::open(&dir.path().join("missing.sqlite")).is_err());
    }
}
