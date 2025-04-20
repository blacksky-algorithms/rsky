use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_types::OAuthClientId;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

#[derive(Clone, Serialize, Deserialize)]
pub struct SignInCredentials {
    pub username: String,
    pub password: String,

    /**
     * If false, the account must not be returned from
     * {@link AccountStore.listDeviceAccounts}. Note that this only makes sense when
     * used with a device ID.
     */
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remember: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_otp: Option<String>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct DeviceAccountInfo {
    pub remembered: bool,
    pub authenticated_at: u64,
    pub authorized_clients: Vec<OAuthClientId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountInfo {
    pub account: Account,
    pub info: DeviceAccountInfo,
}

pub trait AccountStore: Send + Sync {
    fn authenticate_account(
        &self,
        credentials: SignInCredentials,
        device_id: DeviceId,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AccountInfo>, OAuthError>> + Send + Sync + '_>>;
    fn add_authorized_client(
        &self,
        device_id: DeviceId,
        sub: Sub,
        client_id: OAuthClientId,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    fn get_device_account(
        &self,
        device_id: DeviceId,
        sub: Sub,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AccountInfo>, OAuthError>> + Send + Sync + '_>>;
    fn remove_device_account(
        &self,
        device_id: DeviceId,
        sub: Sub,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>>;
    /**
     * @note Only the accounts that where logged in with `remember: true` need to
     * be returned. The others will be ignored.
     */
    fn list_device_accounts(
        &self,
        device_id: DeviceId,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<AccountInfo>, OAuthError>> + Send + Sync + '_>>;
}
