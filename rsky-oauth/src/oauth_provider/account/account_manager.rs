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

//TODO protect against timing attacks
const TIMING_ATTACK_MITIGATION_DELAY: u32 = 400;

pub struct AccountManager {
    pub store: Arc<RwLock<dyn AccountStore>>,
}

impl AccountManager {
    pub fn new(store: Arc<RwLock<dyn AccountStore>>) -> Self {
        Self { store }
    }

    pub async fn sign_in(
        &self,
        credentials: SignInCredentials,
        device_id: DeviceId,
    ) -> Result<AccountInfo, OAuthError> {
        let store = self.store.read().await;
        match store.authenticate_account(credentials, device_id).await {
            Ok(result) => match result {
                None => Err(OAuthError::InvalidRequestError(
                    "Invalid credentials".to_string(),
                )),
                Some(account_info) => Ok(account_info),
            },
            Err(_) => Err(OAuthError::InvalidRequestError(
                "Invalid credentials".to_string(),
            )),
        }
    }

    pub async fn get(&self, device_id: &DeviceId, sub: Sub) -> Result<AccountInfo, OAuthError> {
        let store = self.store.read().await;
        match store.get_device_account(device_id.clone(), sub).await {
            Ok(result) => match result {
                None => Err(OAuthError::InvalidRequestError(
                    "Account not found".to_string(),
                )),
                Some(account_info) => Ok(account_info),
            },
            Err(_) => Err(OAuthError::InvalidRequestError(
                "Account not found".to_string(),
            )),
        }
    }

    pub async fn add_authorized_client(
        &self,
        device_id: DeviceId,
        account: Account,
        client: Client,
        _client_auth: ClientAuth,
    ) {
        // "Loopback" clients are not distinguishable from one another.
        if !is_oauth_client_id_loopback(&client.id) {
            let store = self.store.read().await;
            store
                .add_authorized_client(device_id, account.sub, client.id)
                .await
                .unwrap();
        }
    }

    pub async fn list(&self, device_id: &DeviceId) -> Vec<AccountInfo> {
        let store = self.store.read().await;
        let results = store.list_device_accounts(device_id.clone()).await.unwrap();
        let mut x = Vec::new();
        for res in results {
            if res.info.remembered {
                x.push(res.clone())
            }
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::Audience;
    use crate::oauth_provider::account::account_store::DeviceAccountInfo;
    use crate::oauth_types::OAuthClientId;
    use chrono::Utc;
    use std::future::Future;
    use std::pin::Pin;

    struct TestAccountStore {}

    impl AccountStore for TestAccountStore {
        fn authenticate_account(
            &self,
            credentials: SignInCredentials,
            device_id: DeviceId,
        ) -> Pin<Box<dyn Future<Output = Result<Option<AccountInfo>, OAuthError>> + Send + Sync + '_>>
        {
            let expected_credentials = SignInCredentials {
                username: "username".to_string(),
                password: "password".to_string(),
                remember: None,
                email_otp: None,
            };
            let expected_device_id = DeviceId::new("dev-64976a0a962c4b7521abd679789c44a1").unwrap();
            Box::pin(async move {
                if credentials == expected_credentials && device_id == expected_device_id {
                    Ok(Some(AccountInfo {
                        account: Account {
                            sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                            aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                            preferred_username: None,
                            email: None,
                            email_verified: None,
                            picture: None,
                            name: None,
                        },
                        info: DeviceAccountInfo {
                            remembered: false,
                            authenticated_at: Utc::now(),
                            authorized_clients: vec![],
                        },
                    }))
                } else {
                    panic!()
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
                if device_id == DeviceId::new("dev-64976a0a962c4b7521abd679789c44a3").unwrap() {
                    Ok(())
                } else {
                    panic!()
                }
            })
        }

        fn get_device_account(
            &self,
            device_id: DeviceId,
            sub: Sub,
        ) -> Pin<Box<dyn Future<Output = Result<Option<AccountInfo>, OAuthError>> + Send + Sync + '_>>
        {
            Box::pin(async move {
                if device_id == DeviceId::new("dev-64976a0a962c4b7521abd679789c44a2").unwrap()
                    && sub == Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap()
                {
                    Ok(Some(AccountInfo {
                        account: Account {
                            sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                            aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                            preferred_username: None,
                            email: None,
                            email_verified: None,
                            picture: None,
                            name: None,
                        },
                        info: DeviceAccountInfo {
                            remembered: false,
                            authenticated_at: Utc::now(),
                            authorized_clients: vec![],
                        },
                    }))
                } else {
                    panic!()
                }
            })
        }

        fn remove_device_account(
            &self,
            device_id: DeviceId,
            sub: Sub,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn list_device_accounts(
            &self,
            device_id: DeviceId,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<AccountInfo>, OAuthError>> + Send + Sync + '_>>
        {
            Box::pin(async move {
                if device_id == DeviceId::new("dev-64976a0a962c4b7521abd679789c44a4").unwrap() {
                    let infos = vec![AccountInfo {
                        account: Account {
                            sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                            aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                            preferred_username: None,
                            email: None,
                            email_verified: None,
                            picture: None,
                            name: None,
                        },
                        info: DeviceAccountInfo {
                            remembered: false,
                            authenticated_at: Utc::now(),
                            authorized_clients: vec![],
                        },
                    }];
                    Ok(infos)
                } else {
                    panic!()
                }
            })
        }
    }

    fn create_account_manager() -> AccountManager {
        AccountManager::new(Arc::new(RwLock::new(TestAccountStore {})))
    }

    #[tokio::test]
    async fn test_sign_in() {
        let account_manager = create_account_manager();
        let credentials = SignInCredentials {
            username: "username".to_string(),
            password: "password".to_string(),
            remember: None,
            email_otp: None,
        };
        let device_id = DeviceId::new("dev-64976a0a962c4b7521abd679789c44a1").unwrap();
        let result = account_manager
            .sign_in(credentials, device_id)
            .await
            .unwrap();
        let expected = AccountInfo {
            account: Account {
                sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                preferred_username: None,
                email: None,
                email_verified: None,
                picture: None,
                name: None,
            },
            info: DeviceAccountInfo {
                remembered: false,
                authenticated_at: Utc::now(),
                authorized_clients: vec![],
            },
        };
    }

    #[tokio::test]
    async fn test_get() {
        let account_manager = create_account_manager();
        let device_id = DeviceId::new("dev-64976a0a962c4b7521abd679789c44a2").unwrap();
        let sub = Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap();
        let result = account_manager.get(&device_id, sub).await.unwrap();
        let expected = AccountInfo {
            account: Account {
                sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                preferred_username: None,
                email: None,
                email_verified: None,
                picture: None,
                name: None,
            },
            info: DeviceAccountInfo {
                remembered: false,
                authenticated_at: Utc::now(),
                authorized_clients: vec![],
            },
        };
    }

    #[tokio::test]
    async fn test_add_authorized_client() {
        let account_manager = create_account_manager();
        let device_id = DeviceId::new("dev-64976a0a962c4b7521abd679789c44a3").unwrap();
        let account = Account {
            sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
            aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
            preferred_username: None,
            email: None,
            email_verified: None,
            picture: None,
            name: None,
        };
        let client = Client {
            id: OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json")
                .unwrap(),
            metadata: Default::default(),
            jwks: None,
            info: Default::default(),
        };
        let _client_auth = ClientAuth {
            method: "POST".to_string(),
            alg: "".to_string(),
            kid: "".to_string(),
            jkt: "".to_string(),
        };
        account_manager
            .add_authorized_client(device_id, account, client, _client_auth)
            .await;
    }

    #[tokio::test]
    async fn test_list() {
        let account_manager = create_account_manager();
        let device_id = DeviceId::new("dev-64976a0a962c4b7521abd679789c44a4").unwrap();
        let result = account_manager.list(&device_id).await;
        let expected = vec![];
        assert_eq!(result, expected);
    }
}
