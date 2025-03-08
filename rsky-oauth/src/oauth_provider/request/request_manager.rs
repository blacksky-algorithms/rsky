use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::constants::PAR_EXPIRES_IN;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::request::request_data::{
    is_request_data_authorized, RequestData, RequestDataAuthorized,
};
use crate::oauth_provider::request::request_id::generate_request_id;
use crate::oauth_provider::request::request_info::RequestInfo;
use crate::oauth_provider::request::request_store::UpdateRequestData;
use crate::oauth_provider::request::request_store_memory::RequestStoreMemory;
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_provider::signer::signer::Signer;
use crate::oauth_types::{
    OAuthAuthorizationRequestParameters, OAuthAuthorizationServerMetadata, OAuthRequestUri,
};
use rocket::form::validate::Contains;
use std::time::SystemTime;

pub struct RequestManager {
    store: RequestStoreMemory,
    signer: Signer,
    metadata: OAuthAuthorizationServerMetadata,
    token_max_age: u64,
}

impl RequestManager {
    pub fn new(
        store: RequestStoreMemory,
        signer: Signer,
        metadata: OAuthAuthorizationServerMetadata,
        token_max_age: u64,
    ) -> Self {
        RequestManager {
            store,
            signer,
            metadata,
            token_max_age,
        }
    }

    pub fn create_token_expiry(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_millis() as u64;
        now - self.token_max_age
    }

    pub async fn create_authorization_request(
        &mut self,
        client: &Client,
        client_auth: &ClientAuth,
        input: &OAuthAuthorizationRequestParameters,
        device_id: Option<DeviceId>,
        dpop_jkt: Option<String>,
    ) -> RequestInfo {
        let parameters = self.validate(client, client_auth, input, dpop_jkt).await;
        self.create(client, client_auth, &parameters, device_id)
            .await
    }

    pub async fn create(
        &mut self,
        client: &Client,
        client_auth: &ClientAuth,
        parameters: &OAuthAuthorizationRequestParameters,
        device_id: Option<DeviceId>,
    ) -> RequestInfo {
        unimplemented!()
        // let expires_at: u32;
        // let id = generate_request_id().await;
        //
        // let data = RequestData {
        //     client_id: client.id.clone(),
        //     client_auth: client_auth.clone(),
        //     parameters: parameters.clone(),
        //     expires_at: expires_at,
        //     device_id,
        //     sub: None,
        //     code: None,
        // };
        // self.store.create_request(id.clone(), data).await;
        //
        // let uri = encode_request_uri(id.clone());
        //
        // RequestInfo {
        //     id,
        //     uri: uri,
        //     parameters: parameters.clone(),
        //     expires_at: expires_at,
        //     client_id: client.id.clone(),
        //     client_auth: client_auth.clone(),
        // }
    }

    async fn validate(
        &self,
        client: &Client,
        client_auth: &ClientAuth,
        parameters: &OAuthAuthorizationRequestParameters,
        dpop_jkt: Option<String>,
    ) -> OAuthAuthorizationRequestParameters {
        unimplemented!()
    }

    pub async fn get(
        &mut self,
        uri: &OAuthRequestUri,
        client_id: &ClientId,
        device_id: &DeviceId,
    ) -> Result<RequestInfo, OAuthError> {
        unimplemented!()
    }

    pub async fn set_authorized(
        &mut self,
        client: Client,
        uri: OAuthRequestUri,
        device_id: DeviceId,
        account: Account,
    ) -> Result<Code, OAuthError> {
        unimplemented!()
    }

    /**
     * @note If this method throws an error, any token previously generated from
     * the same `code` **must** me revoked.
     */
    pub async fn find_code(
        &mut self,
        client: Client,
        client_auth: ClientAuth,
        code: String,
    ) -> Result<RequestDataAuthorized, OAuthError> {
        unimplemented!()
    }

    pub async fn delete(&mut self, request_uri: &OAuthRequestUri) {
        unimplemented!()
        // let id = request_uri.request_id();
        // self.store.delete_request(id).await;
    }
}
