use anyhow::{bail, Result};
use diesel::*;
use crate::common::time::{MINUTE, less_than_ago_ms};
use crate::db::establish_connection;
use crate::models::EmailToken;
use crate::models::models::EmailTokenPurpose;

pub async fn assert_valid_token(
    did: &String,
    purpose: EmailTokenPurpose,
    token: &String,
    expiration_len: Option<i32>
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
        let expired = !less_than_ago_ms(res.requested_at, expiration_len);
        if expired {
            bail!("Token is expired")
        }
        Ok(())
    } else {
        bail!("Token is invalid")
    }
}