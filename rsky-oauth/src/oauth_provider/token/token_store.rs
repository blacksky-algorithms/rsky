use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::account::account_store::DeviceAccountInfo;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::token::refresh_token::RefreshToken;
use crate::oauth_provider::token::token_data::TokenData;
use crate::oauth_provider::token::token_id::TokenId;
use chrono::{DateTime, Utc};
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Eq, PartialEq)]
pub struct TokenInfo {
    pub id: TokenId,
    pub data: TokenData,
    pub account: Account,
    pub info: Option<DeviceAccountInfo>,
    pub current_refresh_token: Option<RefreshToken>,
}

pub struct NewTokenData {
    pub client_auth: ClientAuth,
    pub expires_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub trait TokenStore: Send + Sync {
    fn create_token(
        &mut self,
        token_id: TokenId,
        data: TokenData,
        refresh_token: Option<RefreshToken>,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    fn read_token(
        &self,
        token_id: TokenId,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TokenInfo>, OAuthError>> + Send + Sync + '_>>;
    fn delete_token(
        &mut self,
        token_id: TokenId,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    fn rotate_token(
        &mut self,
        token_id: TokenId,
        new_token_id: TokenId,
        new_refresh_token: RefreshToken,
        new_data: NewTokenData,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    /**
     * Find a token by its refresh token. Note that previous refresh tokens
     * should also return the token. The data model is responsible for storing
     * old refresh tokens when a new one is issued.
     */
    fn find_token_by_refresh_token(
        &self,
        refresh_token: RefreshToken,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TokenInfo>, OAuthError>> + Send + Sync + '_>>;
    fn find_token_by_code(
        &self,
        code: Code,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TokenInfo>, OAuthError>> + Send + Sync + '_>>;
}
