use crate::apis::com::atproto::server::get_random_token;
use crate::db::DbConn;
use crate::models::models::EmailTokenPurpose;
use crate::models::EmailToken;
use anyhow::{bail, Result};
use diesel::*;
use rsky_common;
use rsky_common::time::{from_str_to_utc, less_than_ago_s, MINUTE};

pub async fn create_email_token(
    did: &str,
    purpose: EmailTokenPurpose,
    db: &DbConn,
) -> Result<String> {
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;
    let token = get_random_token().to_uppercase();
    let now = rsky_common::now();

    let did = did.to_owned();
    db.run(move |conn| {
        insert_into(EmailTokenSchema::email_token)
            .values((
                EmailTokenSchema::purpose.eq(purpose),
                EmailTokenSchema::did.eq(did),
                EmailTokenSchema::token.eq(&token),
                EmailTokenSchema::requestedAt.eq(&now),
            ))
            .on_conflict((EmailTokenSchema::purpose, EmailTokenSchema::did))
            .do_update()
            .set((
                EmailTokenSchema::token.eq(&token),
                EmailTokenSchema::requestedAt.eq(&now),
            ))
            .execute(conn)?;
        Ok(token)
    })
    .await
}

pub async fn assert_valid_token(
    did: &str,
    purpose: EmailTokenPurpose,
    token: &str,
    expiration_len: Option<i32>,
    db: &DbConn,
) -> Result<()> {
    let expiration_len = expiration_len.unwrap_or(MINUTE * 15);
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;

    let did = did.to_owned();
    let token = token.to_owned();
    let res = db
        .run(move |conn| {
            EmailTokenSchema::email_token
                .filter(EmailTokenSchema::purpose.eq(purpose))
                .filter(EmailTokenSchema::did.eq(did))
                .filter(EmailTokenSchema::token.eq(token.to_uppercase()))
                .select(EmailToken::as_select())
                .first(conn)
                .optional()
        })
        .await?;
    if let Some(res) = res {
        let requested_at = from_str_to_utc(&res.requested_at);
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
    db: &DbConn,
) -> Result<String> {
    let expiration_len = expiration_len.unwrap_or(MINUTE * 15);
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;

    let token = token.to_owned();
    let res = db
        .run(move |conn| {
            EmailTokenSchema::email_token
                .filter(EmailTokenSchema::purpose.eq(purpose))
                .filter(EmailTokenSchema::token.eq(token.to_uppercase()))
                .select(EmailToken::as_select())
                .first(conn)
                .optional()
        })
        .await?;
    if let Some(res) = res {
        let requested_at = from_str_to_utc(&res.requested_at);
        let expired = !less_than_ago_s(requested_at, expiration_len);
        if expired {
            bail!("Token is expired")
        }
        Ok(res.did)
    } else {
        bail!("Token is invalid")
    }
}

pub async fn delete_email_token(did: &str, purpose: EmailTokenPurpose, db: &DbConn) -> Result<()> {
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;
    let did = did.to_owned();
    db.run(move |conn| {
        delete(EmailTokenSchema::email_token)
            .filter(EmailTokenSchema::did.eq(did))
            .filter(EmailTokenSchema::purpose.eq(purpose))
            .execute(conn)
    })
    .await?;
    Ok(())
}

pub async fn delete_all_email_tokens(did: &str, db: &DbConn) -> Result<()> {
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;

    let did = did.to_owned();
    db.run(move |conn| {
        delete(EmailTokenSchema::email_token)
            .filter(EmailTokenSchema::did.eq(did))
            .execute(conn)
    })
    .await?;

    Ok(())
}
