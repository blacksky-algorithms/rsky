use crate::error::OAuthError;
use crate::request::RequestData;
use crate::token::{TokenData, TokenInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// A user account as seen by the OAuth provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub did: String,
    pub handle: Option<String>,
    pub email: Option<String>,
    pub deactivated: bool,
}

/// A device session row (`device` table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceData {
    pub session_id: String,
    pub user_agent: Option<String>,
    pub ip_address: String,
    /// Unix seconds.
    pub last_seen_at: u64,
}

/// Storage backend for the OAuth provider, mirroring the semantics of
/// the upstream PDS `oauth-store`.
#[async_trait::async_trait]
pub trait OAuthStore: Send + Sync {
    // Authorization requests
    async fn create_request(&self, id: &str, data: &RequestData) -> Result<(), OAuthError>;
    async fn read_request(&self, id: &str) -> Result<Option<RequestData>, OAuthError>;
    async fn update_request(&self, id: &str, data: &RequestData) -> Result<(), OAuthError>;
    async fn delete_request(&self, id: &str) -> Result<(), OAuthError>;
    /// Atomically deletes and returns the request bound to `code`.
    async fn consume_request_code(
        &self,
        code: &str,
    ) -> Result<Option<(String, RequestData)>, OAuthError>;

    // Tokens
    async fn create_token(
        &self,
        token_id: &str,
        data: &TokenData,
        refresh_token: Option<&str>,
    ) -> Result<(), OAuthError>;
    async fn read_token(&self, token_id: &str) -> Result<Option<TokenInfo>, OAuthError>;
    /// Must also find tokens by previously-used (rotated) refresh tokens
    /// so replays are detectable.
    async fn find_token_by_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Option<TokenInfo>, OAuthError>;
    async fn find_token_by_code(&self, code: &str) -> Result<Option<TokenInfo>, OAuthError>;
    /// Rotates the token in place: records the old refresh token as used,
    /// swaps in the new ids and advances `updated_at`/`expires_at`.
    async fn rotate_token(
        &self,
        token_id: &str,
        new_token_id: &str,
        new_refresh_token: &str,
        updated_at: u64,
        expires_at: u64,
    ) -> Result<(), OAuthError>;
    async fn delete_token(&self, token_id: &str) -> Result<(), OAuthError>;

    // Accounts
    /// Validates credentials; `Ok(None)` when they don't match an
    /// active account.
    async fn authenticate_account(
        &self,
        identifier: &str,
        password: &str,
    ) -> Result<Option<AccountInfo>, OAuthError>;
    async fn get_account(&self, did: &str) -> Result<Option<AccountInfo>, OAuthError>;

    // Devices
    async fn create_device(&self, device_id: &str, data: &DeviceData) -> Result<(), OAuthError>;
    async fn read_device(&self, device_id: &str) -> Result<Option<DeviceData>, OAuthError>;
    async fn update_device(&self, device_id: &str, data: &DeviceData) -> Result<(), OAuthError>;
    async fn upsert_device_account(&self, device_id: &str, did: &str) -> Result<(), OAuthError>;
    async fn get_device_account(
        &self,
        device_id: &str,
        did: &str,
    ) -> Result<Option<AccountInfo>, OAuthError>;
    async fn list_device_accounts(&self, device_id: &str) -> Result<Vec<AccountInfo>, OAuthError>;
    async fn remove_device_account(&self, device_id: &str, did: &str) -> Result<(), OAuthError>;

    // Authorized clients
    async fn set_authorized_client(
        &self,
        did: &str,
        client_id: &str,
        scope: &str,
    ) -> Result<(), OAuthError>;
    async fn get_authorized_client_scope(
        &self,
        did: &str,
        client_id: &str,
    ) -> Result<Option<String>, OAuthError>;
}

#[derive(Debug, Default)]
struct MemoryState {
    requests: HashMap<String, RequestData>,
    tokens: HashMap<String, (TokenData, Option<String>)>,
    used_refresh_tokens: HashMap<String, String>,
    accounts: HashMap<String, (AccountInfo, String)>,
    devices: HashMap<String, DeviceData>,
    device_accounts: Vec<(String, String)>,
    authorized_clients: HashMap<(String, String), String>,
}

/// In-memory [`OAuthStore`] for tests and embedded use.
#[derive(Debug, Default)]
pub struct MemoryOAuthStore {
    state: Mutex<MemoryState>,
}

impl MemoryOAuthStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an account with a plaintext password (test fixture).
    pub fn add_account(&self, account: AccountInfo, password: &str) {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state
            .accounts
            .insert(account.did.clone(), (account, password.to_string()));
    }

    fn token_info(state: &MemoryState, token_id: &str) -> Option<TokenInfo> {
        state.tokens.get(token_id).map(|(data, refresh)| TokenInfo {
            token_id: token_id.to_string(),
            data: data.clone(),
            current_refresh_token: refresh.clone(),
        })
    }
}

#[async_trait::async_trait]
impl OAuthStore for MemoryOAuthStore {
    async fn create_request(&self, id: &str, data: &RequestData) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.requests.insert(id.to_string(), data.clone());
        Ok(())
    }

    async fn read_request(&self, id: &str) -> Result<Option<RequestData>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state.requests.get(id).cloned())
    }

    async fn update_request(&self, id: &str, data: &RequestData) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        if !state.requests.contains_key(id) {
            return Err(OAuthError::ServerError("unknown request".to_string()));
        }
        state.requests.insert(id.to_string(), data.clone());
        Ok(())
    }

    async fn delete_request(&self, id: &str) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.requests.remove(id);
        Ok(())
    }

    async fn consume_request_code(
        &self,
        code: &str,
    ) -> Result<Option<(String, RequestData)>, OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        let id = state
            .requests
            .iter()
            .find(|(_, data)| data.code.as_deref() == Some(code))
            .map(|(id, _)| id.clone());
        Ok(id.map(|id| {
            let data = state
                .requests
                .remove(&id)
                .expect("request found by code above");
            (id, data)
        }))
    }

    async fn create_token(
        &self,
        token_id: &str,
        data: &TokenData,
        refresh_token: Option<&str>,
    ) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        if let Some(refresh_token) = refresh_token {
            if state.used_refresh_tokens.contains_key(refresh_token) {
                return Err(OAuthError::ServerError(
                    "refresh token already in use".to_string(),
                ));
            }
        }
        state.tokens.insert(
            token_id.to_string(),
            (data.clone(), refresh_token.map(String::from)),
        );
        Ok(())
    }

    async fn read_token(&self, token_id: &str) -> Result<Option<TokenInfo>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(Self::token_info(&state, token_id))
    }

    async fn find_token_by_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Option<TokenInfo>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        if let Some(token_id) = state.used_refresh_tokens.get(refresh_token) {
            return Ok(Self::token_info(&state, token_id));
        }
        let token_id = state
            .tokens
            .iter()
            .find(|(_, (_, refresh))| refresh.as_deref() == Some(refresh_token))
            .map(|(id, _)| id.clone());
        Ok(token_id.and_then(|id| Self::token_info(&state, &id)))
    }

    async fn find_token_by_code(&self, code: &str) -> Result<Option<TokenInfo>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        let token_id = state
            .tokens
            .iter()
            .find(|(_, (data, _))| data.code.as_deref() == Some(code))
            .map(|(id, _)| id.clone());
        Ok(token_id.and_then(|id| Self::token_info(&state, &id)))
    }

    async fn rotate_token(
        &self,
        token_id: &str,
        new_token_id: &str,
        new_refresh_token: &str,
        updated_at: u64,
        expires_at: u64,
    ) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        let Some((mut data, refresh)) = state.tokens.remove(token_id) else {
            return Err(OAuthError::ServerError("unknown token".to_string()));
        };
        if let Some(refresh) = refresh {
            state
                .used_refresh_tokens
                .insert(refresh, new_token_id.to_string());
        }
        // rewire previously-used refresh tokens to the rotated id
        for used_token_id in state.used_refresh_tokens.values_mut() {
            if used_token_id == token_id {
                *used_token_id = new_token_id.to_string();
            }
        }
        data.updated_at = updated_at;
        data.expires_at = expires_at;
        state.tokens.insert(
            new_token_id.to_string(),
            (data, Some(new_refresh_token.to_string())),
        );
        Ok(())
    }

    async fn delete_token(&self, token_id: &str) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.tokens.remove(token_id);
        state
            .used_refresh_tokens
            .retain(|_, used_token_id| used_token_id != token_id);
        Ok(())
    }

    async fn authenticate_account(
        &self,
        identifier: &str,
        password: &str,
    ) -> Result<Option<AccountInfo>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state
            .accounts
            .values()
            .find(|(account, stored_password)| {
                stored_password == password
                    && (account.did == identifier || account.handle.as_deref() == Some(identifier))
            })
            .map(|(account, _)| account.clone()))
    }

    async fn get_account(&self, did: &str) -> Result<Option<AccountInfo>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state.accounts.get(did).map(|(account, _)| account.clone()))
    }

    async fn create_device(&self, device_id: &str, data: &DeviceData) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.devices.insert(device_id.to_string(), data.clone());
        Ok(())
    }

    async fn read_device(&self, device_id: &str) -> Result<Option<DeviceData>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state.devices.get(device_id).cloned())
    }

    async fn update_device(&self, device_id: &str, data: &DeviceData) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        if !state.devices.contains_key(device_id) {
            return Err(OAuthError::ServerError("unknown device".to_string()));
        }
        state.devices.insert(device_id.to_string(), data.clone());
        Ok(())
    }

    async fn upsert_device_account(&self, device_id: &str, did: &str) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        let pair = (device_id.to_string(), did.to_string());
        if !state.device_accounts.contains(&pair) {
            state.device_accounts.push(pair);
        }
        Ok(())
    }

    async fn get_device_account(
        &self,
        device_id: &str,
        did: &str,
    ) -> Result<Option<AccountInfo>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        let linked = state
            .device_accounts
            .iter()
            .any(|(linked_device, linked_did)| linked_device == device_id && linked_did == did);
        Ok(if linked {
            state.accounts.get(did).map(|(account, _)| account.clone())
        } else {
            None
        })
    }

    async fn list_device_accounts(&self, device_id: &str) -> Result<Vec<AccountInfo>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state
            .device_accounts
            .iter()
            .filter(|(linked_device, _)| linked_device == device_id)
            .filter_map(|(_, did)| state.accounts.get(did).map(|(account, _)| account.clone()))
            .collect())
    }

    async fn remove_device_account(&self, device_id: &str, did: &str) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.device_accounts.retain(|(linked_device, linked_did)| {
            !(linked_device == device_id && linked_did == did)
        });
        Ok(())
    }

    async fn set_authorized_client(
        &self,
        did: &str,
        client_id: &str,
        scope: &str,
    ) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state
            .authorized_clients
            .insert((did.to_string(), client_id.to_string()), scope.to_string());
        Ok(())
    }

    async fn get_authorized_client_scope(
        &self,
        did: &str,
        client_id: &str,
    ) -> Result<Option<String>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state
            .authorized_clients
            .get(&(did.to_string(), client_id.to_string()))
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(did: &str) -> AccountInfo {
        AccountInfo {
            did: did.to_string(),
            handle: Some(format!("{}.example.com", &did[8..])),
            email: None,
            deactivated: false,
        }
    }

    fn device() -> DeviceData {
        DeviceData {
            session_id: "ses-1".to_string(),
            user_agent: Some("test-agent".to_string()),
            ip_address: "127.0.0.1".to_string(),
            last_seen_at: 1_700_000_000,
        }
    }

    #[tokio::test]
    async fn device_crud() {
        let store = MemoryOAuthStore::new();
        assert!(store.read_device("dev-1").await.unwrap().is_none());
        assert!(store.update_device("dev-1", &device()).await.is_err());
        store.create_device("dev-1", &device()).await.unwrap();
        assert_eq!(store.read_device("dev-1").await.unwrap(), Some(device()));
        let mut updated = device();
        updated.last_seen_at += 100;
        store.update_device("dev-1", &updated).await.unwrap();
        assert_eq!(store.read_device("dev-1").await.unwrap(), Some(updated));
    }

    #[tokio::test]
    async fn device_accounts_and_authorized_clients() {
        let store = MemoryOAuthStore::new();
        store.add_account(account("did:plc:alice"), "pw");
        store.create_device("dev-1", &device()).await.unwrap();
        assert!(store
            .get_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap()
            .is_none());
        store
            .upsert_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap();
        // upsert is idempotent
        store
            .upsert_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap();
        assert!(store
            .get_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap()
            .is_some());
        assert_eq!(store.list_device_accounts("dev-1").await.unwrap().len(), 1);
        store
            .remove_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap();
        assert!(store
            .list_device_accounts("dev-1")
            .await
            .unwrap()
            .is_empty());

        assert!(store
            .get_authorized_client_scope("did:plc:alice", "client-1")
            .await
            .unwrap()
            .is_none());
        store
            .set_authorized_client("did:plc:alice", "client-1", "atproto")
            .await
            .unwrap();
        assert_eq!(
            store
                .get_authorized_client_scope("did:plc:alice", "client-1")
                .await
                .unwrap()
                .as_deref(),
            Some("atproto")
        );
    }

    #[tokio::test]
    async fn request_and_token_error_branches() {
        use crate::request::RequestData;
        use crate::token::TokenData;
        use crate::types::{AuthorizationRequestParameters, ClientAuth};
        let store = MemoryOAuthStore::new();
        let parameters = AuthorizationRequestParameters {
            client_id: "client-1".to_string(),
            response_type: "code".to_string(),
            redirect_uri: "https://app.example.com/cb".to_string(),
            scope: "atproto".to_string(),
            state: None,
            code_challenge: "challenge".to_string(),
            code_challenge_method: "S256".to_string(),
            login_hint: None,
            prompt: None,
            dpop_jkt: None,
        };
        let request = RequestData {
            client_id: "client-1".to_string(),
            client_auth: ClientAuth::None,
            parameters: parameters.clone(),
            expires_at: 1,
            device_id: None,
            did: None,
            code: None,
        };
        assert!(store.update_request("req-1", &request).await.is_err());
        assert!(store.consume_request_code("cod-x").await.unwrap().is_none());

        let token = TokenData {
            created_at: 0,
            updated_at: 0,
            expires_at: 1,
            client_id: "client-1".to_string(),
            client_auth: ClientAuth::None,
            device_id: None,
            did: "did:plc:alice".to_string(),
            parameters,
            code: None,
        };
        assert!(store
            .rotate_token("tok-1", "tok-2", "ref-2", 1, 2)
            .await
            .is_err());
        store
            .create_token("tok-1", &token, Some("ref-1"))
            .await
            .unwrap();
        store
            .rotate_token("tok-1", "tok-2", "ref-2", 1, 2)
            .await
            .unwrap();
        // creating a token that reuses a rotated-out refresh token fails
        assert!(store
            .create_token("tok-3", &token, Some("ref-1"))
            .await
            .is_err());
        // rotating again rewires the used_refresh_token mapping
        store
            .rotate_token("tok-2", "tok-4", "ref-4", 3, 4)
            .await
            .unwrap();
        let found = store
            .find_token_by_refresh_token("ref-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.token_id, "tok-4");
    }
}
