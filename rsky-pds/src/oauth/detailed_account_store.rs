use crate::account_manager::AccountManager;
use crate::actor_store::ActorStore;
use crate::lexicon::lexicons::ProfileViewBasic;
use crate::read_after_write::viewer::{LocalViewer, LocalViewerCreator};
use rsky_oauth::jwk::Keyset;
use rsky_oauth::oauth_provider::account::account_store::{
    AccountInfo, AccountStore, SignInCredentials,
};
use rsky_oauth::oauth_provider::client::client_id::ClientId;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::oidc::sub::Sub;

/**
 * Although the {@link AccountManager} class implements the {@link AccountStore}
 * interface, the accounts it returns do not contain any profile information
 * (display name, avatar, etc). This is due to the fact that the account manager
 * does not have access to the account's repos. The {@link DetailedAccountStore}
 * is a wrapper around the {@link AccountManager} that enriches the accounts
 * with profile information using the account's repos through the
 * {@link ActorStore}.
 */
pub struct DetailedAccountStore {
    account_manager: AccountManager,
    actor_store: ActorStore,
    local_view: LocalViewerCreator,
}

pub type DetailedAccountStoreCreator = Box<
    dyn Fn(ActorStore, AccountManager, LocalViewerCreator) -> DetailedAccountStore + Send + Sync,
>;

impl DetailedAccountStore {
    pub fn creator() -> DetailedAccountStoreCreator {
        Box::new(
            move |account_manager: AccountManager,
                  actor_store: ActorStore,
                  local_viewer: LocalViewerCreator|
                  -> DetailedAccountStore {
                DetailedAccountStore::new(account_manager, actor_store, local_viewer)
            },
        )
    }

    pub fn new(
        account_manager: AccountManager,
        actor_store: ActorStore,
        local_viewer: LocalViewerCreator,
    ) -> Self {
        Self {
            account_manager,
            actor_store,
            local_view,
        }
    }

    fn get_profile(&self, did: &str) -> Option<ProfileViewBasic> {
        unimplemented!()
    }

    fn enrich_account_info(&self, account_info: AccountInfo) -> AccountInfo {
        let mut enriched_account_info = account_info.clone();
        if account_info.account.picture.is_none() || account_info.account.name.is_none() {
            let profile = self.get_profile(account_info.account.sub.get().as_str());
            match profile {
                None => {}
                Some(profile) => {
                    unimplemented!()
                    // enriched_account_info.account.picture = Some(profile.properties.avatar.format);
                    // enriched_account_info.account.name = Some(profile.properties.display_name.)
                }
            }
        }
        enriched_account_info
    }
}

impl AccountStore for DetailedAccountStore {
    fn authenticate_account(
        &self,
        credentials: SignInCredentials,
        device_id: DeviceId,
    ) -> Option<AccountInfo> {
        let account_info = self
            .account_manager
            .authenticate_account(credentials, device_id);
        match account_info {
            None => None,
            Some(account_info) => Some(self.enrich_account_info(account_info)),
        }
    }

    fn add_authorized_client(&self, device_id: DeviceId, sub: Sub, client_id: ClientId) {
        self.account_manager
            .add_authorized_client(device_id, sub, client_id)
    }

    fn get_device_account(&self, device_id: &DeviceId, sub: Sub) -> Option<AccountInfo> {
        let account_info = self.account_manager.get_device_account(device_id, sub);
        match account_info {
            None => None,
            Some(account_info) => Some(self.enrich_account_info(account_info)),
        }
    }

    fn remove_device_account(&self, device_id: DeviceId, sub: Sub) {
        self.account_manager.remove_device_account(device_id, sub)
    }

    fn list_device_accounts(&self, device_id: &DeviceId) -> Vec<AccountInfo> {
        let account_infos = self.account_manager.list_device_accounts(device_id);
        let mut enriched_account_infos = vec![];
        for account_info in account_infos {
            let enriched_account_info = self.enrich_account_info(account_info);
            enriched_account_infos.push(enriched_account_info);
        }
        enriched_account_infos
    }
}
