use crate::db::DbConn;
use crate::models::models;
use crate::schema::pds::used_refresh_token::dsl as RefreshSchema;
use anyhow::Result;
use diesel::*;
use diesel::{insert_into, OptionalExtension, QueryDsl, RunQueryDsl, SelectableHelper};
use rsky_oauth::oauth_provider::token::refresh_token::RefreshToken;

pub async fn insert_qb(refresh_token: RefreshToken, token_id: u64, db: &DbConn) -> Result<()> {
    db.run(move |conn| {
        let rows: Vec<models::UsedRefreshToken> = vec![models::UsedRefreshToken {
            token_id: token_id.to_string(),
            refresh_token: refresh_token.val(),
        }];
        insert_into(RefreshSchema::used_refresh_token)
            .values(&rows)
            .execute(conn)
    })
    .await?;
    Ok(())
}

pub async fn find_by_token_qb(
    refresh_token: RefreshToken,
    db: &DbConn,
) -> Result<Option<RefreshToken>> {
    let refresh_token = refresh_token.val();
    // db.run(move |conn| {
    //     RefreshSchema::used_refresh_token
    //         .filter(RefreshSchema::refreshToken.eq(refresh_token))
    //         .select(models::UsedRefreshToken::as_select())
    //         .first(conn)
    //         .optional()
    // })
    // .await
    unimplemented!()
}

pub async fn count_qb(refresh_token: RefreshToken, db: &DbConn) -> Result<u64> {
    let refresh_token = refresh_token.val();
    let result = db
        .run(move |conn| {
            RefreshSchema::used_refresh_token
                .filter(RefreshSchema::refreshToken.eq(refresh_token))
                .select(models::UsedRefreshToken::as_select())
                .get_results(conn)
        })
        .await?;
    Ok(result.len() as u64)
}
