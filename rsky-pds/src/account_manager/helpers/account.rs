use crate::db::sqlite::Db;
use anyhow::Result;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use rsky_common::RFC3339_VARIANT;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use rusqlite::{params, ErrorCode, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::ops::Add;
use std::time::SystemTime;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AccountHelperError {
    #[error("UserAlreadyExistsError")]
    UserAlreadyExistsError,
}

pub struct AvailabilityFlags {
    pub include_taken_down: Option<bool>,
    pub include_deactivated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum AccountStatus {
    Active,
    Takendown,
    Suspended,
    Deleted,
    Deactivated,
    Desynchronized,
    Throttled,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FormattedAccountStatus {
    pub active: bool,
    pub status: Option<AccountStatus>,
}

#[derive(Debug)]
pub struct GetAccountAdminStatusOutput {
    pub takedown: StatusAttr,
    pub deactivated: StatusAttr,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActorAccount {
    pub did: String,
    pub handle: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "takedownRef")]
    pub takedown_ref: Option<String>,
    #[serde(rename = "deactivatedAt")]
    pub deactivated_at: Option<String>,
    #[serde(rename = "deleteAfter")]
    pub delete_after: Option<String>,
    pub email: Option<String>,
    #[serde(rename = "invitesDisabled")]
    pub invites_disabled: Option<i16>,
    #[serde(rename = "emailConfirmedAt")]
    pub email_confirmed_at: Option<String>,
}

const SELECT_ACTOR_ACCOUNT: &str = "\
    SELECT actor.did, actor.handle, actor.\"createdAt\", actor.\"takedownRef\", \
    actor.\"deactivatedAt\", actor.\"deleteAfter\", \
    account.email, account.\"emailConfirmedAt\", account.\"invitesDisabled\" \
    FROM actor LEFT JOIN account ON actor.did = account.did";

fn availability_conditions(flags: Option<AvailabilityFlags>) -> String {
    let AvailabilityFlags {
        include_taken_down,
        include_deactivated,
    } = flags.unwrap_or(AvailabilityFlags {
        include_taken_down: Some(false),
        include_deactivated: Some(false),
    });
    let mut conditions = String::new();
    if !include_taken_down.unwrap_or(false) {
        conditions.push_str(" AND actor.\"takedownRef\" IS NULL");
    }
    if !include_deactivated.unwrap_or(false) {
        conditions.push_str(" AND actor.\"deactivatedAt\" IS NULL");
    }
    conditions
}

fn actor_account_from_row(row: &Row) -> Result<ActorAccount, rusqlite::Error> {
    Ok(ActorAccount {
        did: row.get(0)?,
        handle: row.get(1)?,
        created_at: row.get(2)?,
        takedown_ref: row.get(3)?,
        deactivated_at: row.get(4)?,
        delete_after: row.get(5)?,
        email: row.get(6)?,
        email_confirmed_at: row.get(7)?,
        invites_disabled: row.get(8)?,
    })
}

pub fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err.sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation)
    )
}

pub async fn get_account(
    handle_or_did: &str,
    flags: Option<AvailabilityFlags>,
    db: &Db,
) -> Result<Option<ActorAccount>> {
    let handle_or_did = handle_or_did.to_owned();
    let sql = format!(
        "{SELECT_ACTOR_ACCOUNT} WHERE {} = ?1{}",
        if handle_or_did.starts_with("did:") {
            "actor.did"
        } else {
            "actor.handle"
        },
        availability_conditions(flags)
    );
    db.run(move |conn| {
        Ok(conn
            .query_row(&sql, params![handle_or_did], actor_account_from_row)
            .optional()?)
    })
    .await
}

pub async fn get_account_by_email(
    email: &str,
    flags: Option<AvailabilityFlags>,
    db: &Db,
) -> Result<Option<ActorAccount>> {
    let email = email.to_lowercase();
    let sql = format!(
        "{SELECT_ACTOR_ACCOUNT} WHERE account.email = ?1{}",
        availability_conditions(flags)
    );
    db.run(move |conn| {
        Ok(conn
            .query_row(&sql, params![email], actor_account_from_row)
            .optional()?)
    })
    .await
}

pub async fn register_actor(
    did: String,
    handle: String,
    deactivated: Option<bool>,
    db: &Db,
) -> Result<()> {
    let system_time = SystemTime::now();
    let dt: DateTime<UtcOffset> = system_time.into();
    let created_at = format!("{}", dt.format(RFC3339_VARIANT));
    let deactivate_at = match deactivated {
        Some(true) => Some(created_at.clone()),
        _ => None,
    };
    let deactivate_after = match deactivated {
        Some(true) => {
            let exp = dt.add(chrono::Duration::days(3));
            Some(format!("{}", exp.format(RFC3339_VARIANT)))
        }
        _ => None,
    };

    let registered = db
        .run(move |conn| {
            Ok(conn
                .query_row(
                    "INSERT INTO actor (did, handle, \"createdAt\", \"deactivatedAt\", \"deleteAfter\") \
                     VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT (did) DO NOTHING \
                     RETURNING did",
                    params![did, handle, created_at, deactivate_at, deactivate_after],
                    |row| row.get::<_, String>(0),
                )
                .optional()?)
        })
        .await?;
    match registered {
        Some(_) => Ok(()),
        None => Err(anyhow::Error::new(
            AccountHelperError::UserAlreadyExistsError,
        )),
    }
}

pub async fn register_account(did: String, email: String, password: String, db: &Db) -> Result<()> {
    let created_at = rsky_common::now();

    // @TODO record recovery key for bring your own recovery key
    let registered = db
        .run(move |conn| {
            Ok(conn
                .query_row(
                    "INSERT INTO account (did, email, password, \"createdAt\") \
                     VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT (did) DO NOTHING \
                     RETURNING did",
                    params![did, email, password, created_at],
                    |row| row.get::<_, String>(0),
                )
                .optional()?)
        })
        .await?;
    match registered {
        Some(_) => Ok(()),
        None => Err(anyhow::Error::new(
            AccountHelperError::UserAlreadyExistsError,
        )),
    }
}

pub async fn delete_account(did: &str, db: &Db) -> Result<()> {
    let did = did.to_owned();
    db.tx(move |tx| {
        tx.execute("DELETE FROM repo_root WHERE did = ?1", params![did])?;
        tx.execute("DELETE FROM email_token WHERE did = ?1", params![did])?;
        tx.execute("DELETE FROM refresh_token WHERE did = ?1", params![did])?;
        tx.execute("DELETE FROM account WHERE did = ?1", params![did])?;
        tx.execute("DELETE FROM actor WHERE did = ?1", params![did])?;
        Ok(())
    })
    .await
}

pub async fn update_account_takedown_status(
    did: &str,
    takedown: StatusAttr,
    db: &Db,
) -> Result<()> {
    let takedown_ref: Option<String> = match takedown.applied {
        true => match takedown.r#ref {
            Some(takedown_ref) => Some(takedown_ref),
            None => Some(rsky_common::now()),
        },
        false => None,
    };
    let did = did.to_owned();
    db.run(move |conn| {
        conn.execute(
            "UPDATE actor SET \"takedownRef\" = ?1 WHERE did = ?2",
            params![takedown_ref, did],
        )?;
        Ok(())
    })
    .await
}

pub async fn deactivate_account(did: &str, delete_after: Option<String>, db: &Db) -> Result<()> {
    let did = did.to_owned();
    let deactivated_at = rsky_common::now();
    db.run(move |conn| {
        conn.execute(
            "UPDATE actor SET \"deactivatedAt\" = ?1, \"deleteAfter\" = ?2 WHERE did = ?3",
            params![deactivated_at, delete_after, did],
        )?;
        Ok(())
    })
    .await
}

pub async fn activate_account(did: &str, db: &Db) -> Result<()> {
    let did = did.to_owned();
    db.run(move |conn| {
        conn.execute(
            "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
            params![did],
        )?;
        Ok(())
    })
    .await
}

pub async fn update_email(did: &str, email: &str, db: &Db) -> Result<()> {
    let did = did.to_owned();
    let email = email.to_lowercase();
    let res = db
        .run(move |conn| {
            conn.execute(
                "UPDATE account SET email = ?1, \"emailConfirmedAt\" = NULL WHERE did = ?2",
                params![email, did],
            )?;
            Ok(())
        })
        .await;
    match res {
        Ok(_) => Ok(()),
        Err(err) => match err.downcast_ref::<rusqlite::Error>() {
            Some(sqlite_err) if is_unique_violation(sqlite_err) => Err(anyhow::Error::new(
                AccountHelperError::UserAlreadyExistsError,
            )),
            _ => Err(err),
        },
    }
}

pub async fn update_handle(did: &str, handle: &str, db: &Db) -> Result<()> {
    let did = did.to_owned();
    let handle = handle.to_owned();
    let updated = db
        .run(move |conn| {
            Ok(conn.execute(
                "UPDATE actor SET handle = ?1 \
                 WHERE did = ?2 \
                 AND NOT EXISTS (SELECT 1 FROM actor actor2 WHERE actor2.handle = ?1)",
                params![handle, did],
            )?)
        })
        .await?;
    if updated < 1 {
        return Err(anyhow::Error::new(
            AccountHelperError::UserAlreadyExistsError,
        ));
    }
    Ok(())
}

pub async fn set_email_confirmed_at(did: &str, email_confirmed_at: String, db: &Db) -> Result<()> {
    let did = did.to_owned();
    db.run(move |conn| {
        conn.execute(
            "UPDATE account SET \"emailConfirmedAt\" = ?1 WHERE did = ?2",
            params![email_confirmed_at, did],
        )?;
        Ok(())
    })
    .await
}

pub async fn get_account_admin_status(
    did: &str,
    db: &Db,
) -> Result<Option<GetAccountAdminStatusOutput>> {
    let did = did.to_owned();
    let res: Option<(Option<String>, Option<String>)> = db
        .run(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT \"takedownRef\", \"deactivatedAt\" FROM actor WHERE did = ?1",
                    params![did],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?)
        })
        .await?;
    match res {
        None => Ok(None),
        Some(res) => {
            let takedown = match res.0 {
                Some(takedown_ref) => StatusAttr {
                    applied: true,
                    r#ref: Some(takedown_ref),
                },
                None => StatusAttr {
                    applied: false,
                    r#ref: None,
                },
            };
            let deactivated = match res.1 {
                Some(_) => StatusAttr {
                    applied: true,
                    r#ref: None,
                },
                None => StatusAttr {
                    applied: false,
                    r#ref: None,
                },
            };
            Ok(Some(GetAccountAdminStatusOutput {
                takedown,
                deactivated,
            }))
        }
    }
}

pub fn format_account_status(account: Option<ActorAccount>) -> FormattedAccountStatus {
    match account {
        None => FormattedAccountStatus {
            active: false,
            status: Some(AccountStatus::Deleted),
        },
        Some(got) if got.takedown_ref.is_some() => FormattedAccountStatus {
            active: false,
            status: Some(AccountStatus::Takendown),
        },
        Some(got) if got.deactivated_at.is_some() => FormattedAccountStatus {
            active: false,
            status: Some(AccountStatus::Deactivated),
        },
        _ => FormattedAccountStatus {
            active: true,
            status: None,
        },
    }
}
