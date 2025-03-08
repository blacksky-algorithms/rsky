use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::oidc::sub::Sub;
use std::fmt::Debug;

#[derive(Clone)]
pub struct SignInCredentials {
    pub username: String,
    pub password: String,

    /**
     * If false, the account must not be returned from
     * {@link AccountStore.listDeviceAccounts}. Note that this only makes sense when
     * used with a device ID.
     */
    pub remember: Option<bool>,

    pub email_otp: Option<String>,
}

#[derive(Clone)]
pub struct DeviceAccountInfo {
    pub remembered: bool,
    pub authenticated_at: u64,
    pub authorized_clients: Vec<ClientId>,
}

#[derive(Clone)]
pub struct AccountInfo {
    pub account: Account,
    pub info: DeviceAccountInfo,
}

pub struct AccountStore {}

impl AccountStore {
    pub async fn authenticate_account(
        &self,
        credentials: SignInCredentials,
        device_id: DeviceId,
    ) -> Option<AccountInfo> {
        unimplemented!()
    }
    pub async fn add_authorized_client(&self, device_id: DeviceId, sub: Sub, client_id: ClientId) {
        unimplemented!()
    }
    pub async fn get_device_account(&self, device_id: &DeviceId, sub: Sub) -> Option<AccountInfo> {
        unimplemented!()
    }
    pub async fn remove_device_account(&self, device_id: DeviceId, sub: Sub) {
        unimplemented!()
    }
    pub async fn list_device_accounts(&self, device_id: &DeviceId) -> Vec<AccountInfo> {
        unimplemented!()
    }
}

pub fn is_account_store() {
    unimplemented!()
}

pub fn as_account_store() {
    unimplemented!()
}
