use diesel::PgConnection;
use rsky_oauth::oauth_provider::token::refresh_token::RefreshToken;
use rsky_oauth::oauth_provider::token::token_data::TokenData;
use rsky_oauth::oauth_provider::token::token_id::TokenId;

pub fn create_qb(
    conn: &mut PgConnection,
    token_id: TokenId,
    data: TokenData,
    refresh_token: Option<RefreshToken>,
) {
    todo!()
}
