use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::account::account_store::{AccountInfo, AccountStore, SignInCredentials};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_types::is_oauth_client_id_loopback;
use std::sync::Arc;
use tokio::sync::RwLock;

const TIMING_ATTACK_MITIGATION_DELAY: u32 = 400;

pub struct AccountManager {
    pub store: Arc<RwLock<dyn AccountStore>>,
}

impl AccountManager {
    pub fn new(store: Arc<RwLock<dyn AccountStore>>) -> Self {
        Self { store }
    }

    pub fn sign_in(
        &self,
        credentials: SignInCredentials,
        device_id: DeviceId,
    ) -> Result<AccountInfo, OAuthError> {
        match self
            .store
            .blocking_read()
            .authenticate_account(credentials, device_id)
        {
            None => Err(OAuthError::InvalidRequestError(
                "Invalid credentials".to_string(),
            )),
            Some(account_info) => Ok(account_info),
        }
    }

    pub fn get(&self, device_id: &DeviceId, sub: Sub) -> Result<AccountInfo, OAuthError> {
        match self
            .store
            .blocking_read()
            .get_device_account(device_id, sub)
        {
            None => Err(OAuthError::InvalidRequestError(
                "Account not found".to_string(),
            )),
            Some(account_info) => Ok(account_info),
        }
    }

    pub fn add_authorized_client(
        &self,
        device_id: DeviceId,
        account: Account,
        client: Client,
        _client_auth: ClientAuth,
    ) {
        // "Loopback" clients are not distinguishable from one another.
        if !is_oauth_client_id_loopback(&client.id) {
            self.store
                .blocking_write()
                .add_authorized_client(device_id, account.sub, client.id);
        }
    }

    pub fn list(&self, device_id: &DeviceId) -> Vec<AccountInfo> {
        let results = self.store.blocking_read().list_device_accounts(device_id);
        let mut x = Vec::new();
        for res in results {
            if res.info.remembered {
                x.push(res.clone())
            }
        }
        x
    }
}
