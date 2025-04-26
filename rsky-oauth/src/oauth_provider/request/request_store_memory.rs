use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::request::request_data::RequestData;
use crate::oauth_provider::request::request_id::RequestId;
use crate::oauth_provider::request::request_store::{
    FoundRequestResult, RequestStore, UpdateRequestData,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

pub struct RequestStoreMemory {
    requests: BTreeMap<RequestId, RequestData>,
}

impl Default for RequestStoreMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestStoreMemory {
    pub fn new() -> Self {
        Self {
            requests: Default::default(),
        }
    }
}

impl RequestStore for RequestStoreMemory {
    fn create_request(
        &mut self,
        id: RequestId,
        data: RequestData,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
        Box::pin(async move {
            self.requests.insert(id, data);
            Ok(())
        })
    }

    fn read_request(
        &self,
        id: &RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<Option<RequestData>, OAuthError>> + Send + Sync + '_>>
    {
        let id = id.clone();
        Box::pin(async move {
            match self.requests.get(&id) {
                None => Ok(None),
                Some(data) => Ok(Some(data.clone())),
            }
        })
    }

    fn update_request(
        &mut self,
        id: RequestId,
        data: UpdateRequestData,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
        let data = data;
        Box::pin(async move {
            let current = match self.requests.get(&id) {
                None => return Err(OAuthError::RuntimeError("Request Not Found".to_string())),
                Some(res) => res,
            };
            let device_id = match data.device_id {
                None => current.device_id.clone(),
                Some(device_id) => Some(device_id),
            };
            let sub = match data.sub {
                None => current.sub.clone(),
                Some(sub) => Some(sub),
            };
            let code = match data.code {
                None => current.code.clone(),
                Some(code) => Some(code),
            };
            let new_data = RequestData {
                client_id: data.client_id.unwrap_or(current.client_id.clone()),
                client_auth: data.client_auth.unwrap_or(current.client_auth.clone()),
                parameters: data.parameters.unwrap_or(current.parameters.clone()),
                expires_at: data.expires_at.unwrap_or(current.expires_at),
                device_id,
                sub,
                code,
            };
            self.requests.insert(id, new_data);
            Ok(())
        })
    }

    fn delete_request(
        &mut self,
        id: RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
        Box::pin(async move {
            self.requests.remove(&id);
            Ok(())
        })
    }

    fn find_request_by_code(
        &self,
        code: Code,
    ) -> Pin<Box<dyn Future<Output = Option<FoundRequestResult>> + Send + Sync + '_>> {
        Box::pin(async move {
            for (id, data) in &self.requests {
                if let Some(found_code) = &data.code {
                    if found_code.clone() == code {
                        return Some(FoundRequestResult {
                            id: id.clone(),
                            data: data.clone(),
                        });
                    }
                }
            }
            None
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::oauth_provider::client::client_auth::ClientAuth;
    use crate::oauth_provider::request::code::Code;
    use crate::oauth_provider::request::request_data::RequestData;
    use crate::oauth_provider::request::request_id::RequestId;
    use crate::oauth_provider::request::request_store::{RequestStore, UpdateRequestData};
    use crate::oauth_provider::request::request_store_memory::RequestStoreMemory;
    use crate::oauth_types::{
        OAuthAuthorizationRequestParameters, OAuthClientId, OAuthResponseType,
    };
    use chrono::Utc;

    fn create_request_store() -> RequestStoreMemory {
        RequestStoreMemory::new()
    }

    #[tokio::test]
    async fn test_create_request() {
        let mut request_store = create_request_store();
        let id = RequestId::generate();
        let data: RequestData = RequestData {
            client_id: OAuthClientId::new("client").unwrap(),
            client_auth: ClientAuth::new(None),
            parameters: OAuthAuthorizationRequestParameters {
                client_id: OAuthClientId::new("client").unwrap(),
                state: None,
                redirect_uri: None,
                scope: None,
                response_type: OAuthResponseType::Code,
                code_challenge: None,
                code_challenge_method: None,
                dpop_jkt: None,
                response_mode: None,
                nonce: None,
                max_age: None,
                claims: None,
                login_hint: None,
                ui_locales: None,
                id_token_hint: None,
                display: None,
                prompt: None,
                authorization_details: None,
            },
            expires_at: Utc::now(),
            device_id: None,
            sub: None,
            code: None,
        };
        request_store.create_request(id, data).await.unwrap();
    }

    #[tokio::test]
    async fn test_read_request() {
        let mut request_store = create_request_store();
        let id = RequestId::generate();
        let data: RequestData = RequestData {
            client_id: OAuthClientId::new("client").unwrap(),
            client_auth: ClientAuth::new(None),
            parameters: OAuthAuthorizationRequestParameters {
                client_id: OAuthClientId::new("client").unwrap(),
                state: None,
                redirect_uri: None,
                scope: None,
                response_type: OAuthResponseType::Code,
                code_challenge: None,
                code_challenge_method: None,
                dpop_jkt: None,
                response_mode: None,
                nonce: None,
                max_age: None,
                claims: None,
                login_hint: None,
                ui_locales: None,
                id_token_hint: None,
                display: None,
                prompt: None,
                authorization_details: None,
            },
            expires_at: Utc::now(),
            device_id: None,
            sub: None,
            code: None,
        };
        request_store
            .create_request(id.clone(), data.clone())
            .await
            .unwrap();
        let result = request_store.read_request(&id).await.unwrap().unwrap();
        assert_eq!(data, result);
    }

    #[tokio::test]
    async fn test_update_request() {
        let mut request_store = create_request_store();
        let id = RequestId::generate();
        let data: RequestData = RequestData {
            client_id: OAuthClientId::new("client").unwrap(),
            client_auth: ClientAuth::new(None),
            parameters: OAuthAuthorizationRequestParameters {
                client_id: OAuthClientId::new("client").unwrap(),
                state: None,
                redirect_uri: None,
                scope: None,
                response_type: OAuthResponseType::Code,
                code_challenge: None,
                code_challenge_method: None,
                dpop_jkt: None,
                response_mode: None,
                nonce: None,
                max_age: None,
                claims: None,
                login_hint: None,
                ui_locales: None,
                id_token_hint: None,
                display: None,
                prompt: None,
                authorization_details: None,
            },
            expires_at: Utc::now(),
            device_id: None,
            sub: None,
            code: None,
        };
        request_store
            .create_request(id.clone(), data.clone())
            .await
            .unwrap();
        let update_data = UpdateRequestData {
            client_id: None,
            client_auth: None,
            parameters: None,
            expires_at: None,
            device_id: None,
            sub: None,
            code: None,
        };
        request_store.update_request(id, update_data).await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_request() {
        let mut request_store = create_request_store();
        let id = RequestId::generate();
        request_store.delete_request(id).await.unwrap();
    }

    #[tokio::test]
    async fn test_find_request_by_code() {
        let mut request_store = create_request_store();
        let id = RequestId::generate();
        let code = Code::generate();
        let data: RequestData = RequestData {
            client_id: OAuthClientId::new("client").unwrap(),
            client_auth: ClientAuth::new(None),
            parameters: OAuthAuthorizationRequestParameters {
                client_id: OAuthClientId::new("client").unwrap(),
                state: None,
                redirect_uri: None,
                scope: None,
                response_type: OAuthResponseType::Code,
                code_challenge: None,
                code_challenge_method: None,
                dpop_jkt: None,
                response_mode: None,
                nonce: None,
                max_age: None,
                claims: None,
                login_hint: None,
                ui_locales: None,
                id_token_hint: None,
                display: None,
                prompt: None,
                authorization_details: None,
            },
            expires_at: Utc::now(),
            device_id: None,
            sub: None,
            code: Some(code.clone()),
        };
        request_store
            .create_request(id.clone(), data)
            .await
            .unwrap();
        let request = request_store
            .find_request_by_code(code.clone())
            .await
            .unwrap();
        assert_eq!(request.id, id)
    }
}
