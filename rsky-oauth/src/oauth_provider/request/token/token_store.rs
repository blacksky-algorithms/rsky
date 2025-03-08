use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::account::account_store::DeviceAccountInfo;
use crate::oauth_provider::request::token::refresh_token::RefreshToken;
use crate::oauth_provider::request::token::token_data::TokenData;
use crate::oauth_provider::request::token::token_id::TokenId;

pub struct TokenInfo {
    pub id: TokenId,
    pub data: TokenData,
    pub account: Account,
    pub info: Option<DeviceAccountInfo>,
    pub current_refresh_token: Option<RefreshToken>,
}

pub struct NewTokenData {}
