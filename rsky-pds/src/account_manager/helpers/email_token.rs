use crate::apis::com::atproto::server::get_random_token;
use crate::db::sqlite::Db;
use crate::models::models::EmailTokenPurpose;
use crate::models::EmailToken;
use anyhow::{bail, Result};
use rsky_common::time::{from_str_to_utc, less_than_ago_s, MINUTE};
use rusqlite::{params, OptionalExtension, Row};

pub(crate) fn email_token_from_row(row: &Row) -> Result<EmailToken, rusqlite::Error> {
    let purpose: String = row.get(0)?;
    Ok(EmailToken {
        purpose: EmailTokenPurpose::from_str(&purpose).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("invalid email token purpose: {purpose}").into(),
            )
        })?,
        did: row.get(1)?,
        token: row.get(2)?,
        requested_at: row.get(3)?,
    })
}

pub async fn create_email_token(did: &str, purpose: EmailTokenPurpose, db: &Db) -> Result<String> {
    let token = get_random_token().to_uppercase();
    let now = rsky_common::now();

    let did = did.to_owned();
    let stored_token = token.clone();
    db.run(move |conn| {
        conn.execute(
            "INSERT INTO email_token (purpose, did, token, \"requestedAt\") \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (purpose, did) DO UPDATE SET \
             token = excluded.token, \"requestedAt\" = excluded.\"requestedAt\"",
            params![purpose.as_str(), did, stored_token, now],
        )?;
        Ok(())
    })
    .await?;
    Ok(token)
}

pub async fn assert_valid_token(
    did: &str,
    purpose: EmailTokenPurpose,
    token: &str,
    expiration_len: Option<i32>,
    db: &Db,
) -> Result<()> {
    let expiration_len = expiration_len.unwrap_or(MINUTE * 15);

    let did = did.to_owned();
    let token = token.to_uppercase();
    let res = db
        .run(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT purpose, did, token, \"requestedAt\" FROM email_token \
                     WHERE purpose = ?1 AND did = ?2 AND token = ?3",
                    params![purpose.as_str(), did, token],
                    email_token_from_row,
                )
                .optional()?)
        })
        .await?;
    if let Some(res) = res {
        let requested_at = from_str_to_utc(&res.requested_at)?;
        let expired = !less_than_ago_s(requested_at, expiration_len);
        if expired {
            bail!("Token is expired")
        }
        Ok(())
    } else {
        bail!("Token is invalid")
    }
}

pub async fn assert_valid_token_and_find_did(
    purpose: EmailTokenPurpose,
    token: &str,
    expiration_len: Option<i32>,
    db: &Db,
) -> Result<String> {
    let expiration_len = expiration_len.unwrap_or(MINUTE * 15);

    let token = token.to_uppercase();
    let res = db
        .run(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT purpose, did, token, \"requestedAt\" FROM email_token \
                     WHERE purpose = ?1 AND token = ?2",
                    params![purpose.as_str(), token],
                    email_token_from_row,
                )
                .optional()?)
        })
        .await?;
    if let Some(res) = res {
        let requested_at = from_str_to_utc(&res.requested_at)?;
        let expired = !less_than_ago_s(requested_at, expiration_len);
        if expired {
            bail!("Token is expired")
        }
        Ok(res.did)
    } else {
        bail!("Token is invalid")
    }
}

pub async fn delete_email_token(did: &str, purpose: EmailTokenPurpose, db: &Db) -> Result<()> {
    let did = did.to_owned();
    db.run(move |conn| {
        conn.execute(
            "DELETE FROM email_token WHERE did = ?1 AND purpose = ?2",
            params![did, purpose.as_str()],
        )?;
        Ok(())
    })
    .await
}

pub async fn delete_all_email_tokens(did: &str, db: &Db) -> Result<()> {
    let did = did.to_owned();
    db.run(move |conn| {
        conn.execute("DELETE FROM email_token WHERE did = ?1", params![did])?;
        Ok(())
    })
    .await
}
