use crate::db::DbConn;
use anyhow::{anyhow, bail, Result};
use diesel::{update, PgConnection, RunQueryDsl};
use rsky_oauth::oauth_provider::token::refresh_token::RefreshToken;
use rsky_oauth::oauth_provider::token::token_data::TokenData;
use rsky_oauth::oauth_provider::token::token_id::TokenId;
use rsky_oauth::oauth_provider::token::token_store::TokenInfo;
// pub fn create_qb(
//     conn: &mut PgConnection,
//     token_id: TokenId,
//     data: TokenData,
//     refresh_token: Option<RefreshToken>,
// ) -> Result<()> {
//     use crate::schema::pds::account::dsl as AccountSchema;
//
//     update(AccountSchema::account)
//         .filter(AccountSchema::did.eq(opts.did))
//         .set(AccountSchema::password.eq(opts.password_encrypted))
//         .execute(conn)?;
//     Ok(())
// }

pub struct FindByQbOpts {
    pub id: Option<String>,
    pub code: Option<String>,
    pub token_id: Option<String>,
    pub current_refresh_token: Option<String>,
}

pub fn find_by_qb(db: &DbConn, opts: FindByQbOpts) -> Result<Option<TokenInfo>> {
    unimplemented!()
    // use crate::schema::pds::account::dsl as AccountSchema;
    //
    // update(AccountSchema::account)
    //     .filter(AccountSchema::did.eq(opts.did))
    //     .set(AccountSchema::password.eq(opts.password_encrypted))
    //     .execute(conn)?;
    // Ok(())
}
