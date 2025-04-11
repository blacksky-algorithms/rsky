use crate::db::DbConn;
use anyhow::Result;
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

pub async fn find_by_qb(db: &DbConn, opts: FindByQbOpts) -> Result<Option<TokenInfo>> {
    unimplemented!()
    // use crate::schema::pds::token::dsl as TokenSchema;
    //
    // if opts.current_refresh_token.is_none()
    //     && opts.token_id.is_none()
    //     && opts.code.is_none()
    //     && opts.id.is_none()
    // {
    //     return Err(anyhow::Error::new("At least one search parameter is required"))
    // }
    //
    // let res = db.run(|conn| {
    //     let mut builder = TokenSchema::token;
    //     if let Some(id) = opts.id {
    //         builder = builder.filter(TokenSchema::id.eq(id));
    //     }
    //     if let Some(code) = opts.code {
    //         builder = builder.filter(TokenSchema::code.eq(code));
    //     }
    //     if let Some(token_id) = opts.token_id {
    //         builder = builder.filter(TokenSchema::token_id.eq(token_id));
    //     }
    //     if let Some(current_refresh_token) = opts.current_refresh_token {
    //         builder = builder.filter(TokenSchema::current_refresh_token.eq(current_refresh_token));
    //     }
    //     builder.select(models::Token::as_select()).get_results(conn)
    // })
    // .await?;
    // let did = did.to_owned();
    // let res: Vec<models::InviteCode> = db
    //     .run(move |conn| {
    //         InviteCodeSchema::invite_code
    //             .filter(InviteCodeSchema::forAccount.eq(did))
    //             .select(models::InviteCode::as_select())
    //             .get_results(conn)
    //     })
    //     .await?;
    //
    // let codes: Vec<String> = res.iter().map(|row| row.code.clone()).collect();
    // let mut uses = get_invite_codes_uses_v2(codes, db).await?;
    // Ok(res
    //     .into_iter()
    //     .map(|row| CodeDetail {
    //         code: row.code.clone(),
    //         available: row.available_uses,
    //         disabled: row.disabled == 1,
    //         for_account: row.for_account,
    //         created_by: row.created_by,
    //         created_at: row.created_at,
    //         uses: mem::take(uses.get_mut(&row.code).unwrap_or(&mut Vec::new())),
    //     })
    //     .collect::<Vec<CodeDetail>>())
}

pub fn remove_qb(db: &DbConn, token_id: TokenId) -> Result<()> {
    unimplemented!()
    // use crate::schema::pds::token as TokenSchema;
    // delete(TokenSchema::token)
    //     .filter(TokenSchema::token_id.eq(token_id))
    //     .execute(db)?;
    // Ok(())
}
