use crate::account_manager::AccountManager;
use crate::actor_store::ActorStore;
use crate::read_after_write::viewer::{LocalViewer, LocalViewerCreator};
use rsky_lexicon::app::bsky::actor::ProfileViewBasic;
use rsky_oauth::oauth_provider::account::account_store::{
    AccountInfo, AccountStore, SignInCredentials,
};
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_types::OAuthClientId;
use std::future::Future;
use std::pin::Pin;

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
        unimplemented!()
        // Box::new(
        //     move |account_manager: AccountManager,
        //           actor_store: ActorStore,
        //           local_viewer: LocalViewerCreator|
        //           -> DetailedAccountStore {
        //         DetailedAccountStore::new(account_manager, actor_store, local_viewer)
        //     },
        // )
    }

    pub fn new(
        account_manager: AccountManager,
        actor_store: ActorStore,
        local_viewer: LocalViewerCreator,
    ) -> Self {
        Self {
            account_manager,
            actor_store,
            local_view: local_viewer,
        }
    }

    async fn get_profile(&self, did: &str) -> Option<ProfileViewBasic> {
        unimplemented!()
        //local_viewer.get_profile_basic().await.unwrap()
    }

    async fn enrich_account_info(&self, account_info: AccountInfo) -> AccountInfo {
        let mut enriched_account_info = account_info.clone();
        if account_info.account.picture.is_none() || account_info.account.name.is_none() {
            let profile = self
                .get_profile(account_info.account.sub.get().as_str())
                .await;
            match profile {
                None => {}
                Some(profile) => {
                    enriched_account_info.account.picture = profile.avatar;
                    enriched_account_info.account.name = profile.display_name;
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
    ) -> Pin<Box<dyn Future<Output = Result<Option<AccountInfo>, OAuthError>> + Send + Sync + '_>>
    {
        Box::pin(async move {
            let result = self
                .account_manager
                .authenticate_account(credentials, device_id)
                .await?;
            match result {
                None => Ok(None),
                Some(account_info) => Ok(Some(self.enrich_account_info(account_info).await)),
            }
        })
    }

    fn add_authorized_client(
        &self,
        device_id: DeviceId,
        sub: Sub,
        client_id: OAuthClientId,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
        Box::pin(async move {
            self.account_manager
                .add_authorized_client(device_id, sub, client_id)
                .await
        })
    }

    fn get_device_account(
        &self,
        device_id: DeviceId,
        sub: Sub,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AccountInfo>, OAuthError>> + Send + Sync + '_>>
    {
        let device_id = device_id.clone();
        Box::pin(async move {
            let result = self
                .account_manager
                .get_device_account(device_id, sub)
                .await?;
            match result {
                None => Ok(None),
                Some(account_info) => Ok(Some(self.enrich_account_info(account_info).await)),
            }
        })
    }

    fn remove_device_account(
        &self,
        device_id: DeviceId,
        sub: Sub,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
        Box::pin(async move {
            self.account_manager
                .remove_device_account(device_id, sub)
                .await
        })
    }

    fn list_device_accounts(
        &self,
        device_id: DeviceId,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<AccountInfo>, OAuthError>> + Send + Sync + '_>>
    {
        let device_id = device_id.clone();
        Box::pin(async move {
            let account_infos = self.account_manager.list_device_accounts(device_id).await?;
            let mut enriched_account_infos = vec![];
            for account_info in account_infos {
                let enriched_account_info = self.enrich_account_info(account_info).await;
                enriched_account_infos.push(enriched_account_info);
            }
            Ok(enriched_account_infos)
        })
    }
}
