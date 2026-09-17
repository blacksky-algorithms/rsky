use crate::error::OAuthError;
use crate::request::RequestData;
use crate::token::{TokenData, TokenInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

pub use crate::device::{AUTHENTICATION_MAX_AGE, DEVICE_TOUCH_INTERVAL, EPHEMERAL_SESSION_MAX_AGE};

/// A user account as seen by the OAuth provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub did: String,
    pub handle: Option<String>,
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
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

/// An account signed in on a device (`account_device` joined with `device`).
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceAccount {
    pub device_id: String,
    pub account: AccountInfo,
    pub device: DeviceData,
    /// Unix seconds.
    pub created_at: u64,
    /// Unix seconds; the last successful authentication on this device.
    pub updated_at: u64,
}

pub(crate) fn device_session_changed() -> OAuthError {
    OAuthError::InvalidRequest("device session changed".to_string())
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
    /// swaps in the new ids, advances `updated_at`/`expires_at` and, when
    /// given, replaces the granted scope.
    async fn rotate_token(
        &self,
        token_id: &str,
        new_token_id: &str,
        new_refresh_token: &str,
        updated_at: u64,
        expires_at: u64,
        scope: Option<&str>,
    ) -> Result<(), OAuthError>;
    async fn delete_token(&self, token_id: &str) -> Result<(), OAuthError>;
    /// Every token issued to `did`, newest first.
    async fn list_account_tokens(&self, did: &str) -> Result<Vec<TokenInfo>, OAuthError>;
    async fn remove_tokens_by_did(&self, did: &str) -> Result<(), OAuthError>;

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
    /// Refreshes the device metadata without touching its session id.
    async fn touch_device(
        &self,
        device_id: &str,
        user_agent: Option<&str>,
        ip_address: &str,
        last_seen_at: u64,
    ) -> Result<(), OAuthError>;
    /// Compare-and-set of the device session id; fails with
    /// `InvalidRequest("device session changed")` when the device does not
    /// currently carry `expected_session_id`.
    async fn rotate_device_session(
        &self,
        device_id: &str,
        expected_session_id: &str,
        new_session_id: &str,
    ) -> Result<(), OAuthError>;
    /// Links `did` to the device, refreshing `updated_at` when already linked.
    async fn upsert_device_account(
        &self,
        device_id: &str,
        did: &str,
        now: u64,
    ) -> Result<(), OAuthError>;
    /// Rotates the device session and links the account in one atomic step:
    /// a failed compare-and-set writes nothing.
    async fn authenticate_device_account(
        &self,
        device_id: &str,
        expected_session_id: &str,
        new_session_id: &str,
        did: &str,
        now: u64,
    ) -> Result<(), OAuthError>;
    async fn get_device_account(
        &self,
        device_id: &str,
        did: &str,
    ) -> Result<Option<DeviceAccount>, OAuthError>;
    /// One consistent read: the device must carry exactly `session_id` and
    /// the account must be linked to it.
    async fn get_device_account_for_session(
        &self,
        device_id: &str,
        session_id: &str,
        did: &str,
    ) -> Result<Option<DeviceAccount>, OAuthError>;
    async fn list_device_accounts(&self, device_id: &str)
        -> Result<Vec<DeviceAccount>, OAuthError>;
    async fn list_account_devices(&self, did: &str) -> Result<Vec<DeviceAccount>, OAuthError>;
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
    async fn delete_authorized_clients(&self, did: &str) -> Result<(), OAuthError>;
}

#[derive(Debug, Clone, Copy, Default)]
struct Membership {
    created_at: u64,
    updated_at: u64,
}

#[derive(Debug, Default)]
struct MemoryState {
    requests: HashMap<String, RequestData>,
    tokens: HashMap<String, (TokenData, Option<String>)>,
    /// Insertion order of token ids, so listings are stable.
    token_order: Vec<String>,
    used_refresh_tokens: HashMap<String, String>,
    accounts: HashMap<String, (AccountInfo, String)>,
    devices: HashMap<String, DeviceData>,
    device_accounts: HashMap<(String, String), Membership>,
    authorized_clients: HashMap<(String, String), String>,
}

impl MemoryState {
    fn device_account(&self, device_id: &str, did: &str) -> Option<DeviceAccount> {
        let membership = self
            .device_accounts
            .get(&(device_id.to_string(), did.to_string()))?;
        let device = self.devices.get(device_id)?;
        let (account, _) = self.accounts.get(did)?;
        Some(DeviceAccount {
            device_id: device_id.to_string(),
            account: account.clone(),
            device: device.clone(),
            created_at: membership.created_at,
            updated_at: membership.updated_at,
        })
    }

    /// Memberships matching `filter`, most recently authenticated first.
    fn device_accounts_where(&self, filter: impl Fn(&str, &str) -> bool) -> Vec<DeviceAccount> {
        let mut found: Vec<DeviceAccount> = self
            .device_accounts
            .keys()
            .filter(|(device_id, did)| filter(device_id, did))
            .filter_map(|(device_id, did)| self.device_account(device_id, did))
            .collect();
        found.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.account.did.cmp(&b.account.did))
                .then_with(|| a.device_id.cmp(&b.device_id))
        });
        found
    }

    fn rotate_device_session(
        &mut self,
        device_id: &str,
        expected_session_id: &str,
        new_session_id: &str,
    ) -> Result<(), OAuthError> {
        match self.devices.get_mut(device_id) {
            Some(device) if device.session_id == expected_session_id => {
                device.session_id = new_session_id.to_string();
                Ok(())
            }
            _ => Err(device_session_changed()),
        }
    }

    fn upsert_device_account(&mut self, device_id: &str, did: &str, now: u64) {
        self.device_accounts
            .entry((device_id.to_string(), did.to_string()))
            .and_modify(|membership| membership.updated_at = now)
            .or_insert(Membership {
                created_at: now,
                updated_at: now,
            });
    }
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
        state.token_order.push(token_id.to_string());
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
        scope: Option<&str>,
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
        for ordered in state.token_order.iter_mut() {
            if ordered == token_id {
                *ordered = new_token_id.to_string();
            }
        }
        data.updated_at = updated_at;
        data.expires_at = expires_at;
        if let Some(scope) = scope {
            data.scope = Some(scope.to_string());
        }
        state.tokens.insert(
            new_token_id.to_string(),
            (data, Some(new_refresh_token.to_string())),
        );
        Ok(())
    }

    async fn delete_token(&self, token_id: &str) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.tokens.remove(token_id);
        state.token_order.retain(|ordered| ordered != token_id);
        state
            .used_refresh_tokens
            .retain(|_, used_token_id| used_token_id != token_id);
        Ok(())
    }

    async fn list_account_tokens(&self, did: &str) -> Result<Vec<TokenInfo>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state
            .token_order
            .iter()
            .rev()
            .filter_map(|token_id| Self::token_info(&state, token_id))
            .filter(|token| token.data.did == did)
            .collect())
    }

    async fn remove_tokens_by_did(&self, did: &str) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        let removed: Vec<String> = state
            .tokens
            .iter()
            .filter(|(_, (data, _))| data.did == did)
            .map(|(id, _)| id.clone())
            .collect();
        for token_id in &removed {
            state.tokens.remove(token_id);
        }
        state
            .token_order
            .retain(|ordered| !removed.contains(ordered));
        state
            .used_refresh_tokens
            .retain(|_, used_token_id| !removed.contains(used_token_id));
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

    async fn touch_device(
        &self,
        device_id: &str,
        user_agent: Option<&str>,
        ip_address: &str,
        last_seen_at: u64,
    ) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        let Some(device) = state.devices.get_mut(device_id) else {
            return Err(OAuthError::ServerError("unknown device".to_string()));
        };
        device.user_agent = user_agent.map(String::from);
        device.ip_address = ip_address.to_string();
        device.last_seen_at = last_seen_at;
        Ok(())
    }

    async fn rotate_device_session(
        &self,
        device_id: &str,
        expected_session_id: &str,
        new_session_id: &str,
    ) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.rotate_device_session(device_id, expected_session_id, new_session_id)
    }

    async fn upsert_device_account(
        &self,
        device_id: &str,
        did: &str,
        now: u64,
    ) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.upsert_device_account(device_id, did, now);
        Ok(())
    }

    async fn authenticate_device_account(
        &self,
        device_id: &str,
        expected_session_id: &str,
        new_session_id: &str,
        did: &str,
        now: u64,
    ) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state.rotate_device_session(device_id, expected_session_id, new_session_id)?;
        state.upsert_device_account(device_id, did, now);
        Ok(())
    }

    async fn get_device_account(
        &self,
        device_id: &str,
        did: &str,
    ) -> Result<Option<DeviceAccount>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state.device_account(device_id, did))
    }

    async fn get_device_account_for_session(
        &self,
        device_id: &str,
        session_id: &str,
        did: &str,
    ) -> Result<Option<DeviceAccount>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state
            .device_account(device_id, did)
            .filter(|found| found.device.session_id == session_id))
    }

    async fn list_device_accounts(
        &self,
        device_id: &str,
    ) -> Result<Vec<DeviceAccount>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state.device_accounts_where(|linked_device, _| linked_device == device_id))
    }

    async fn list_account_devices(&self, did: &str) -> Result<Vec<DeviceAccount>, OAuthError> {
        let state = self.state.lock().expect("memory store lock poisoned");
        Ok(state.device_accounts_where(|_, linked_did| linked_did == did))
    }

    async fn remove_device_account(&self, device_id: &str, did: &str) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state
            .device_accounts
            .remove(&(device_id.to_string(), did.to_string()));
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

    async fn delete_authorized_clients(&self, did: &str) -> Result<(), OAuthError> {
        let mut state = self.state.lock().expect("memory store lock poisoned");
        state
            .authorized_clients
            .retain(|(linked_did, _), _| linked_did != did);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AuthorizationRequestParameters, ClientAuth};

    const NOW: u64 = 1_700_000_000;

    fn account(did: &str) -> AccountInfo {
        AccountInfo {
            did: did.to_string(),
            handle: Some(format!("{}.example.com", &did[8..])),
            email: None,
            email_verified: false,
            deactivated: false,
        }
    }

    fn device() -> DeviceData {
        DeviceData {
            session_id: "ses-1".to_string(),
            user_agent: Some("test-agent".to_string()),
            ip_address: "127.0.0.1".to_string(),
            last_seen_at: NOW,
        }
    }

    fn parameters() -> AuthorizationRequestParameters {
        AuthorizationRequestParameters {
            client_id: "client-1".to_string(),
            response_type: "code".to_string(),
            redirect_uri: "https://app.example.com/cb".to_string(),
            scope: "atproto".to_string(),
            state: None,
            code_challenge: "challenge".to_string(),
            code_challenge_method: "S256".to_string(),
            login_hint: None,
            response_mode: None,
            prompt: None,
            dpop_jkt: None,
        }
    }

    fn token(did: &str) -> TokenData {
        TokenData {
            created_at: 0,
            updated_at: 0,
            expires_at: 1,
            client_id: "client-1".to_string(),
            client_auth: ClientAuth::None,
            device_id: None,
            did: did.to_string(),
            parameters: parameters(),
            code: None,
            scope: None,
        }
    }

    #[tokio::test]
    async fn device_crud() {
        let store = MemoryOAuthStore::new();
        assert!(store.read_device("dev-1").await.unwrap().is_none());
        assert!(store.update_device("dev-1", &device()).await.is_err());
        assert!(store
            .touch_device("dev-1", None, "10.0.0.1", NOW)
            .await
            .is_err());
        store.create_device("dev-1", &device()).await.unwrap();
        assert_eq!(store.read_device("dev-1").await.unwrap(), Some(device()));
        let mut updated = device();
        updated.last_seen_at += 100;
        store.update_device("dev-1", &updated).await.unwrap();
        assert_eq!(store.read_device("dev-1").await.unwrap(), Some(updated));

        // touching refreshes the metadata and leaves the session id alone
        store
            .touch_device("dev-1", Some("other-agent"), "10.0.0.1", NOW + 200)
            .await
            .unwrap();
        let touched = store.read_device("dev-1").await.unwrap().unwrap();
        assert_eq!(touched.session_id, "ses-1");
        assert_eq!(touched.user_agent.as_deref(), Some("other-agent"));
        assert_eq!(touched.ip_address, "10.0.0.1");
        assert_eq!(touched.last_seen_at, NOW + 200);
    }

    #[tokio::test]
    async fn session_rotation_is_compare_and_set() {
        let store = MemoryOAuthStore::new();
        assert_eq!(
            store
                .rotate_device_session("dev-1", "ses-1", "ses-2")
                .await
                .unwrap_err(),
            device_session_changed()
        );
        store.create_device("dev-1", &device()).await.unwrap();
        assert_eq!(
            store
                .rotate_device_session("dev-1", "ses-stale", "ses-2")
                .await
                .unwrap_err(),
            device_session_changed()
        );
        store
            .rotate_device_session("dev-1", "ses-1", "ses-2")
            .await
            .unwrap();
        assert_eq!(
            store
                .read_device("dev-1")
                .await
                .unwrap()
                .unwrap()
                .session_id,
            "ses-2"
        );
        // a touch from a request holding the pre-rotation snapshot cannot
        // restore the old secret
        store
            .touch_device("dev-1", Some("test-agent"), "127.0.0.1", NOW + 1)
            .await
            .unwrap();
        assert_eq!(
            store
                .read_device("dev-1")
                .await
                .unwrap()
                .unwrap()
                .session_id,
            "ses-2"
        );
    }

    #[tokio::test]
    async fn authenticate_device_account_is_atomic() {
        let store = MemoryOAuthStore::new();
        store.add_account(account("did:plc:alice"), "pw");
        store.create_device("dev-1", &device()).await.unwrap();

        // a failed compare-and-set writes nothing
        assert_eq!(
            store
                .authenticate_device_account("dev-1", "ses-stale", "ses-2", "did:plc:alice", NOW)
                .await
                .unwrap_err(),
            device_session_changed()
        );
        assert!(store
            .get_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .read_device("dev-1")
                .await
                .unwrap()
                .unwrap()
                .session_id,
            "ses-1"
        );

        store
            .authenticate_device_account("dev-1", "ses-1", "ses-2", "did:plc:alice", NOW)
            .await
            .unwrap();
        let linked = store
            .get_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(linked.device.session_id, "ses-2");
        assert_eq!(linked.created_at, NOW);
        assert_eq!(linked.updated_at, NOW);

        // the old secret no longer authenticates the membership; the new one does
        assert!(store
            .get_device_account_for_session("dev-1", "ses-1", "did:plc:alice")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .get_device_account_for_session("dev-1", "ses-2", "did:plc:alice")
            .await
            .unwrap()
            .is_some());
        assert!(store
            .get_device_account_for_session("dev-1", "ses-2", "did:plc:bob")
            .await
            .unwrap()
            .is_none());

        // re-authenticating keeps created_at and advances updated_at; a
        // failed CAS afterwards leaves the timestamps untouched
        store
            .authenticate_device_account("dev-1", "ses-2", "ses-3", "did:plc:alice", NOW + 50)
            .await
            .unwrap();
        assert!(store
            .authenticate_device_account("dev-1", "ses-2", "ses-4", "did:plc:alice", NOW + 90)
            .await
            .is_err());
        let linked = store
            .get_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(linked.created_at, NOW);
        assert_eq!(linked.updated_at, NOW + 50);
    }

    #[tokio::test]
    async fn device_accounts_and_authorized_clients() {
        let store = MemoryOAuthStore::new();
        store.add_account(account("did:plc:alice"), "pw");
        store.add_account(account("did:plc:bob"), "pw");
        store.create_device("dev-1", &device()).await.unwrap();
        store.create_device("dev-2", &device()).await.unwrap();
        assert!(store
            .get_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap()
            .is_none());
        store
            .upsert_device_account("dev-1", "did:plc:alice", NOW)
            .await
            .unwrap();
        // upsert is idempotent apart from the timestamp
        store
            .upsert_device_account("dev-1", "did:plc:alice", NOW + 5)
            .await
            .unwrap();
        let linked = store
            .get_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(linked.created_at, NOW);
        assert_eq!(linked.updated_at, NOW + 5);
        assert_eq!(linked.device_id, "dev-1");
        assert_eq!(linked.account, account("did:plc:alice"));

        // a membership for an unknown account or device is invisible
        store
            .upsert_device_account("dev-1", "did:plc:ghost", NOW)
            .await
            .unwrap();
        store
            .upsert_device_account("dev-ghost", "did:plc:bob", NOW)
            .await
            .unwrap();
        assert!(store
            .get_device_account("dev-1", "did:plc:ghost")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .list_account_devices("did:plc:bob")
            .await
            .unwrap()
            .is_empty());

        // listings are ordered by the most recent authentication
        store
            .upsert_device_account("dev-1", "did:plc:bob", NOW + 10)
            .await
            .unwrap();
        store
            .upsert_device_account("dev-2", "did:plc:alice", NOW + 1)
            .await
            .unwrap();
        let on_device: Vec<String> = store
            .list_device_accounts("dev-1")
            .await
            .unwrap()
            .into_iter()
            .map(|linked| linked.account.did)
            .collect();
        assert_eq!(on_device, vec!["did:plc:bob", "did:plc:alice"]);
        let alice_devices: Vec<String> = store
            .list_account_devices("did:plc:alice")
            .await
            .unwrap()
            .into_iter()
            .map(|linked| linked.device_id)
            .collect();
        assert_eq!(alice_devices, vec!["dev-1", "dev-2"]);
        // equal timestamps fall back to the account, then the device
        store
            .upsert_device_account("dev-2", "did:plc:alice", NOW + 10)
            .await
            .unwrap();
        store
            .upsert_device_account("dev-2", "did:plc:bob", NOW + 10)
            .await
            .unwrap();
        let tied: Vec<(String, String)> = store
            .list_device_accounts("dev-2")
            .await
            .unwrap()
            .into_iter()
            .map(|linked| (linked.account.did, linked.device_id))
            .collect();
        assert_eq!(
            tied,
            vec![
                ("did:plc:alice".to_string(), "dev-2".to_string()),
                ("did:plc:bob".to_string(), "dev-2".to_string()),
            ]
        );
        let bob_devices: Vec<String> = store
            .list_account_devices("did:plc:bob")
            .await
            .unwrap()
            .into_iter()
            .map(|linked| linked.device_id)
            .collect();
        assert_eq!(bob_devices, vec!["dev-1", "dev-2"]);

        store
            .remove_device_account("dev-1", "did:plc:alice")
            .await
            .unwrap();
        assert_eq!(store.list_device_accounts("dev-1").await.unwrap().len(), 1);

        assert!(store
            .get_authorized_client_scope("did:plc:alice", "client-1")
            .await
            .unwrap()
            .is_none());
        store
            .set_authorized_client("did:plc:alice", "client-1", "atproto")
            .await
            .unwrap();
        store
            .set_authorized_client("did:plc:bob", "client-1", "atproto")
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
        store
            .delete_authorized_clients("did:plc:alice")
            .await
            .unwrap();
        assert!(store
            .get_authorized_client_scope("did:plc:alice", "client-1")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .get_authorized_client_scope("did:plc:bob", "client-1")
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn account_token_listing_and_removal() {
        let store = MemoryOAuthStore::new();
        store
            .create_token("tok-a1", &token("did:plc:alice"), Some("ref-a1"))
            .await
            .unwrap();
        store
            .create_token("tok-b1", &token("did:plc:bob"), Some("ref-b1"))
            .await
            .unwrap();
        store
            .create_token("tok-a2", &token("did:plc:alice"), None)
            .await
            .unwrap();
        let ids: Vec<String> = store
            .list_account_tokens("did:plc:alice")
            .await
            .unwrap()
            .into_iter()
            .map(|token| token.token_id)
            .collect();
        assert_eq!(ids, vec!["tok-a2", "tok-a1"]);

        // rotation keeps the listing position under the new id
        store
            .rotate_token("tok-a1", "tok-a3", "ref-a3", 1, 2, Some("atproto narrowed"))
            .await
            .unwrap();
        let listed = store.list_account_tokens("did:plc:alice").await.unwrap();
        assert_eq!(listed[1].token_id, "tok-a3");
        assert_eq!(listed[1].data.scope.as_deref(), Some("atproto narrowed"));

        store.remove_tokens_by_did("did:plc:alice").await.unwrap();
        assert!(store
            .list_account_tokens("did:plc:alice")
            .await
            .unwrap()
            .is_empty());
        assert!(store
            .find_token_by_refresh_token("ref-a1")
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .list_account_tokens("did:plc:bob")
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn request_and_token_error_branches() {
        let store = MemoryOAuthStore::new();
        let request = RequestData {
            client_id: "client-1".to_string(),
            client_auth: ClientAuth::None,
            parameters: parameters(),
            expires_at: 1,
            device_id: None,
            did: None,
            code: None,
        };
        assert!(store.update_request("req-1", &request).await.is_err());
        assert!(store.consume_request_code("cod-x").await.unwrap().is_none());

        let token = token("did:plc:alice");
        assert!(store
            .rotate_token("tok-1", "tok-2", "ref-2", 1, 2, None)
            .await
            .is_err());
        store
            .create_token("tok-1", &token, Some("ref-1"))
            .await
            .unwrap();
        store
            .rotate_token("tok-1", "tok-2", "ref-2", 1, 2, None)
            .await
            .unwrap();
        assert!(store
            .read_token("tok-2")
            .await
            .unwrap()
            .unwrap()
            .data
            .scope
            .is_none());
        // creating a token that reuses a rotated-out refresh token fails
        assert!(store
            .create_token("tok-3", &token, Some("ref-1"))
            .await
            .is_err());
        // rotating again rewires the used_refresh_token mapping
        store
            .rotate_token("tok-2", "tok-4", "ref-4", 3, 4, None)
            .await
            .unwrap();
        let found = store
            .find_token_by_refresh_token("ref-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.token_id, "tok-4");
    }

    #[test]
    fn account_info_defaults_email_verified() {
        let parsed: AccountInfo = serde_json::from_str(
            r#"{"did":"did:plc:alice","handle":null,"email":null,"deactivated":false}"#,
        )
        .unwrap();
        assert!(!parsed.email_verified);
    }
}
