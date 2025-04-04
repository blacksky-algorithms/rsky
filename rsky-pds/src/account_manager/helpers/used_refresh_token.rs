use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::db::DbConn;
use anyhow::Result;
use diesel::{insert_into, OptionalExtension, QueryDsl, RunQueryDsl};
use rsky_oauth::oauth_provider::token::refresh_token::RefreshToken;

pub fn insert_qb(refresh_token: RefreshToken, token_id: u64, db: &DbConn) -> Result<()> {
    unimplemented!()
    // use crate::schema::pds::used_refresh_token::dsl as UsedRefreshTokenSchema;
    //
    // db.run(move |conn| {
    //     insert_into(UsedRefreshTokenSchema::used_refresh_token)
    //         .values((
    //             UsedRefreshTokenSchema::token_id.eq(token_id),
    //             UsedRefreshTokenSchema::refresh_token.eq(refresh_token),
    //         ))
    //         .on_conflict_do_nothing()
    //         .execute(conn)?;
    // })
    // .await
}

pub fn select_used_refresh_token_qb(flags: Option<AvailabilityFlags>) -> BoxedQuery<'static> {
    unimplemented!()
    // let AvailabilityFlags {
    //     include_taken_down,
    //     include_deactivated,
    // } = flags.unwrap_or_else(|| AvailabilityFlags {
    //     include_taken_down: Some(false),
    //     include_deactivated: Some(false),
    // });
    // let include_taken_down = include_taken_down.unwrap_or(false);
    // let include_deactivated = include_deactivated.unwrap_or(false);
    //
    // let mut builder = ActorSchema::actor
    //     .left_join(AccountSchema::account.on(ActorSchema::did.eq(AccountSchema::did)))
    //     .into_boxed();
    // if !include_taken_down {
    //     builder = builder.filter(ActorSchema::takedownRef.is_null());
    // }
    // if !include_deactivated {
    //     builder = builder.filter(ActorSchema::deactivatedAt.is_null());
    // }
    // builder
}

pub fn find_by_token_qb(refresh_token: RefreshToken, db: &DbConn) -> Result<Option<RefreshToken>> {
    unimplemented!()
    // use crate::schema::pds::used_refresh_token::dsl as UsedRefreshTokenSchema;
    //
    // let found = db
    //     .run(move |conn| {
    //         UsedRefreshTokenSchema::used_refresh_token
    //             .select((
    //                 UsedRefreshTokenSchema::token_id,
    //                 UsedRefreshTokenSchema::refresh_token,
    //             ))
    //             .filter(UsedRefreshTokenSchema::refresh_token.eq(refresh_token))
    //             .first::<(
    //                 String,
    //                 String,
    //             )>(conn)
    //             .map(|res| RefreshToken::new(res.0))
    //             .optional()
    //     })
    //     .await?;
    // Ok(found)
}

pub fn count_qb(refresh_token: RefreshToken, db: &DbConn) -> Result<u64> {
    unimplemented!()
    // use crate::schema::pds::used_refresh_token::dsl as UsedRefreshTokenSchema;
    //
    // db.run(move |conn| {
    //     insert_into(UsedRefreshTokenSchema::used_refresh_token)
    //         .values((
    //             UsedRefreshTokenSchema::token_id.eq(token_id),
    //             UsedRefreshTokenSchema::refresh_token.eq(refresh_token),
    //         ))
    //         .on_conflict_do_nothing()
    //         .execute(conn)?;
    // })
    // .await
}
