use crate::apis::com::atproto::server::get_random_token;
use crate::common;
use crate::common::time::{from_str_to_utc, less_than_ago_ms, MINUTE};
use crate::db::establish_connection;
use crate::models::models::EmailTokenPurpose;
use crate::models::EmailToken;
use anyhow::{bail, Result};
use diesel::*;

pub async fn create_email_token(did: &String, purpose: EmailTokenPurpose) -> Result<String> {
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;
    let conn = &mut establish_connection()?;
    let token = get_random_token().to_uppercase();
    let now = common::now();

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
}

pub async fn assert_valid_token(
    did: &String,
    purpose: EmailTokenPurpose,
    token: &String,
    expiration_len: Option<i32>,
) -> Result<()> {
    let expiration_len = expiration_len.unwrap_or(MINUTE * 15);
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;
    let conn = &mut establish_connection()?;

    let res = EmailTokenSchema::email_token
        .filter(EmailTokenSchema::purpose.eq(purpose))
        .filter(EmailTokenSchema::did.eq(did))
        .filter(EmailTokenSchema::token.eq(token.to_uppercase()))
        .select(EmailToken::as_select())
        .first(conn)
        .optional()?;
    if let Some(res) = res {
        let requested_at = from_str_to_utc(&res.requested_at);
        let expired = !less_than_ago_ms(requested_at, expiration_len);
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
    token: &String,
    expiration_len: Option<i32>,
) -> Result<String> {
    let expiration_len = expiration_len.unwrap_or(MINUTE * 15);
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;
    let conn = &mut establish_connection()?;

    let res = EmailTokenSchema::email_token
        .filter(EmailTokenSchema::purpose.eq(purpose))
        .filter(EmailTokenSchema::token.eq(token.to_uppercase()))
        .select(EmailToken::as_select())
        .first(conn)
        .optional()?;
    if let Some(res) = res {
        let requested_at = from_str_to_utc(&res.requested_at);
        let expired = !less_than_ago_ms(requested_at, expiration_len);
        if expired {
            bail!("Token is expired")
        }
        Ok(res.did)
    } else {
        bail!("Token is invalid")
    }
}

pub async fn delete_email_token(did: &String, purpose: EmailTokenPurpose) -> Result<()> {
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;
    let conn = &mut establish_connection()?;

    delete(EmailTokenSchema::email_token)
        .filter(EmailTokenSchema::did.eq(did))
        .filter(EmailTokenSchema::purpose.eq(purpose))
        .execute(conn)?;
    Ok(())
}

pub async fn delete_all_email_tokens(did: &String) -> Result<()> {
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;
    let conn = &mut establish_connection()?;

    delete(EmailTokenSchema::email_token)
        .filter(EmailTokenSchema::did.eq(did))
        .execute(conn)?;
    Ok(())
}
