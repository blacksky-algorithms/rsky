use crate::account_manager::helpers::account::{select_account_qb, AvailabilityFlags};
use crate::account_manager::helpers::invite::{get_invite_codes_uses_v2, CodeDetail};
use crate::db::DbConn;
use crate::schema::pds::account::{dsl as AccountSchema, email};
use crate::schema::pds::device_account::dsl as DeviceAccountSchema;
use crate::schema::pds::token::dsl as TokenSchema;
use anyhow::Result;
use diesel::{
    delete, insert_into, update, BoolExpressionMethods, ExpressionMethods, JoinOnDsl,
    NullableExpressionMethods, QueryDsl, RunQueryDsl,
};
use rsky_oauth::oauth_provider::token::refresh_token::RefreshToken;
use rsky_oauth::oauth_provider::token::token_data::TokenData;
use rsky_oauth::oauth_provider::token::token_id::TokenId;
use rsky_oauth::oauth_provider::token::token_store::TokenInfo;
use std::mem;

fn select_token_info_qb() {
    unimplemented!()
    // let mut x = select_account_qb(Some(AvailabilityFlags {
    //     include_taken_down: None,
    //     include_deactivated: Some(true),
    // }));
    // x = x.inner_join(TokenSchema::token.on(AccountSchema::did.eq(TokenSchema::did)));
    // x = x.left_join(
    //     DeviceAccountSchema::device_account.on(DeviceAccountSchema::did
    //         .eq(TokenSchema::did)
    //         .and(DeviceAccountSchema::deviceId.eq(TokenSchema::deviceId))),
    // );
    // x
}

pub async fn create_qb(
    db: &DbConn,
    token_id: TokenId,
    data: TokenData,
    refresh_token: Option<RefreshToken>,
) -> Result<()> {
    // let token_id = token_id.val();
    // db.run(move |conn| {
    //     insert_into(TokenSchema::token)
    //         .values((
    //             TokenSchema::tokenId.eq(token_id),
    //             TokenSchema::createdAt.eq(email),
    //             TokenSchema::expiresAt.eq(data.expires_at),
    //             TokenSchema::updatedAt.eq(data.updated_at),
    //             TokenSchema::clientId.eq(data.client_id),
    //             TokenSchema::clientAuth.eq(data.client_auth),
    //             TokenSchema::deviceId.eq(data.device_id),
    //             TokenSchema::did.eq(data.sub),
    //             TokenSchema::parameters.eq(data.parameters),
    //             TokenSchema::details.eq(data.details),
    //             TokenSchema::code.eq(data.code),
    //             TokenSchema::currentRefreshToken.eq(refresh_token),
    //         ))
    //         .execute(conn)
    // }).await?;
    // Ok(())
    unimplemented!()
}

pub struct FindByQbOpts {
    pub id: Option<String>,
    pub code: Option<String>,
    pub token_id: Option<String>,
    pub current_refresh_token: Option<String>,
}

pub async fn find_by_qb(db: &DbConn, opts: FindByQbOpts) -> Result<Option<TokenInfo>> {
    // if opts.current_refresh_token.is_none()
    //     && opts.token_id.is_none()
    //     && opts.code.is_none()
    //     && opts.id.is_none()
    // {
    //     return Err(anyhow::Error::new(
    //         "At least one search parameter is required",
    //     ));
    // }
    //
    // let mut x = select_account_qb(Some(AvailabilityFlags {
    //     include_taken_down: None,
    //     include_deactivated: Some(true),
    // }));
    // let y = select_token_info_qb();
    // x = x.inner_join(TokenSchema::token.on(AccountSchema::did.eq(TokenSchema::did)));
    // x = x.left_join(
    //     DeviceAccountSchema::device_account.on(DeviceAccountSchema::did
    //         .eq(TokenSchema::did)
    //         .and(DeviceAccountSchema::deviceId.eq(TokenSchema::deviceId))),
    // );
    // x
    // let res = db
    //     .run(|conn| {
    //         let mut builder = TokenSchema::token;
    //         if let Some(id) = opts.id {
    //             builder = builder.filter(TokenSchema::id.eq(id));
    //         }
    //         if let Some(code) = opts.code {
    //             builder = builder.filter(TokenSchema::code.eq(code));
    //         }
    //         if let Some(token_id) = opts.token_id {
    //             builder = builder.filter(TokenSchema::token_id.eq(token_id));
    //         }
    //         if let Some(current_refresh_token) = opts.current_refresh_token {
    //             builder =
    //                 builder.filter(TokenSchema::current_refresh_token.eq(current_refresh_token));
    //         }
    //         builder.select(models::Token::as_select()).get_results(conn)
    //     })
    //     .await?;
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
    unimplemented!()
}

pub async fn remove_qb(db: &DbConn, token_id: TokenId) -> Result<()> {
    unimplemented!()
    // delete(TokenSchema::token)
    //     .filter(TokenSchema::token_id.eq(token_id))
    //     .execute(db)?;
    // Ok(())
}

pub async fn for_rotate(db: &DbConn, token_id: TokenId) -> Result<(String, String)> {
    unimplemented!()
    // let token_id = token_id.val();
    // let result = db.run(move |conn| {
    //     TokenSchema::token
    //         .filter(TokenSchema::tokenId.eq(token_id))
    //         .filter(TokenSchema::currentRefreshToken.is_not_null())
    //         .select((TokenSchema::id, TokenSchema::currentRefreshToken.assume_not_null()))
    //         .first::<(String, String)>(conn)
    // }).await;
    // result
}
