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
                None => return Err(OAuthError::RuntimeError("test".to_string())),
                Some(res) => res,
            };
            let new_data = RequestData {
                client_id: data.client_id.unwrap_or(current.client_id.clone()),
                client_auth: data.client_auth.unwrap_or(current.client_auth.clone()),
                parameters: data.parameters.unwrap_or(current.parameters.clone()),
                expires_at: data.expires_at.unwrap_or(current.expires_at.clone()),
                device_id: Some(data.device_id.unwrap_or(current.device_id.clone().unwrap())),
                sub: Some(data.sub.unwrap_or(current.sub.clone().unwrap())),
                code: Some(data.code.unwrap_or(current.code.clone().unwrap())),
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
