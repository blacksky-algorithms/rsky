use crate::db::DbConn;
use anyhow::{anyhow, bail, Result};
use diesel::{delete, update, PgConnection, RunQueryDsl};
use rsky_oauth::oauth_provider::token::refresh_token::RefreshToken;
use rsky_oauth::oauth_provider::token::token_data::TokenData;
use rsky_oauth::oauth_provider::token::token_id::TokenId;
use rsky_oauth::oauth_provider::token::token_store::TokenInfo;

pub fn create_qb(
    db: &DbConn,
    token_id: TokenId,
    data: TokenData,
    refresh_token: Option<RefreshToken>,
) -> Result<()> {
    unimplemented!()
    // use crate::schema::pds::account::dsl as AccountSchema;
    //
    // update(AccountSchema::account)
    //     .filter(AccountSchema::did.eq(opts.did))
    //     .set(AccountSchema::password.eq(opts.password_encrypted))
    //     .execute(conn)?;
    // Ok(())
}

pub struct FindByQbOpts {
    pub id: Option<String>,
    pub code: Option<String>,
    pub token_id: Option<String>,
    pub current_refresh_token: Option<String>,
}

pub fn find_by_qb(db: &DbConn, opts: FindByQbOpts) -> Result<Option<TokenInfo>> {
    unimplemented!()
    // use crate::schema::pds::token as TokenSchema;
    // let x = update(TokenSchema::token)
    //     .filter(AccountSchema::did.eq(opts.did))
    //     .set(AccountSchema::password.eq(opts.password_encrypted))
    //     .execute(db)?;
    // Ok(())
}

pub fn remove_qb(db: &DbConn, token_id: TokenId) -> Result<()> {
    unimplemented!()
    // use crate::schema::pds::token as TokenSchema;
    // delete(TokenSchema::token)
    //     .filter(TokenSchema::token_id.eq(token_id))
    //     .execute(db)?;
    // Ok(())
}
