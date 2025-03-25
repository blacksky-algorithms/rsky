use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::constants::{AUTHORIZATION_INACTIVITY_TIMEOUT, PAR_EXPIRES_IN};
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::request::code::{generate_code, Code};
use crate::oauth_provider::request::request_data::{
    is_request_data_authorized, RequestData, RequestDataAuthorized,
};
use crate::oauth_provider::request::request_id::{generate_request_id, RequestId};
use crate::oauth_provider::request::request_info::RequestInfo;
use crate::oauth_provider::request::request_store::{RequestStore, UpdateRequestData};
use crate::oauth_provider::request::request_uri::{decode_request_uri, encode_request_uri};
use crate::oauth_provider::signer::signer::Signer;
use crate::oauth_types::{
    OAuthAuthorizationRequestParameters, OAuthAuthorizationServerMetadata, OAuthRequestUri,
};
use rocket::form::validate::Contains;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

pub struct RequestManager {
    store: Arc<RwLock<dyn RequestStore>>,
    signer: Signer,
    metadata: OAuthAuthorizationServerMetadata,
    // hooks: OAuthHooks,
    token_max_age: u64,
}

impl RequestManager {
    pub fn new(
        store: Arc<RwLock<dyn RequestStore>>,
        signer: Signer,
        metadata: OAuthAuthorizationServerMetadata,
        // hooks: OAuthHooks,
        token_max_age: u64,
    ) -> Self {
        RequestManager {
            store,
            signer,
            metadata,
            // hooks,
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
        client: Client,
        client_auth: ClientAuth,
        input: OAuthAuthorizationRequestParameters,
        device_id: Option<DeviceId>,
        dpop_jkt: Option<String>,
    ) -> Result<RequestInfo, OAuthError> {
        let parameters = self
            .validate(client.clone(), client_auth.clone(), input, dpop_jkt)
            .await?;
        self.create(client, client_auth, parameters, device_id)
            .await
    }

    pub async fn create(
        &mut self,
        client: Client,
        client_auth: ClientAuth,
        parameters: OAuthAuthorizationRequestParameters,
        device_id: Option<DeviceId>,
    ) -> Result<RequestInfo, OAuthError> {
        unimplemented!()
        // let expires_at = SystemTime::now()
        //     .duration_since(SystemTime::UNIX_EPOCH)
        //     .expect("timestamp in micros since UNIX epoch")
        //     .as_micros() as u64
        //     + PAR_EXPIRES_IN;
        // let id = generate_request_id().await;
        //
        // let data = RequestData {
        //     client_id: client.id.clone(),
        //     client_auth: client_auth.clone(),
        //     parameters: parameters.clone(),
        //     expires_at: expires_at as u32,
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
        //     uri,
        //     parameters: parameters.clone(),
        //     expires_at,
        //     client_id: client.id.clone(),
        //     client_auth: client_auth.clone(),
        // }
    }

    async fn validate(
        &self,
        client: Client,
        client_auth: ClientAuth,
        parameters: OAuthAuthorizationRequestParameters,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthAuthorizationRequestParameters, OAuthError> {
        unimplemented!()
        // // -------------------------------
        // // Validate unsupported parameters
        // // -------------------------------
        //
        // // for (const k of [
        // //     // Known unsupported OIDC parameters
        // //     'claims',
        // //     'id_token_hint',
        // //     'nonce', // note that OIDC "nonce" is redundant with PKCE
        // // ] as const) {
        // //     if (parameters[k] !== undefined) {
        // //         throw new InvalidParametersError(
        // //             parameters,
        // //             `Unsupported "${k}" parameter`,
        // //         )
        // //     }
        // // }
        //
        // // -----------------------
        // // Validate against server
        // // -----------------------
        //
        // if !self
        //     .metadata
        //     .response_types_supported
        //     .clone()
        //     .unwrap()
        //     .contains(parameters.response_type)
        // {
        //     return Err(OAuthError::AccessDeniedError(
        //         "Unsupported response_type".to_string(),
        //     ));
        // }
        //
        // if parameters.response_type.includes_code()
        //     && !self
        //         .metadata
        //         .grant_types_supported
        //         .clone()
        //         .unwrap()
        //         .contains("authorization_code")
        // {
        //     return Err(OAuthError::AccessDeniedError(
        //         "Unsupported grant_type authorization_code".to_string(),
        //     ));
        // }
        //
        // if let Some(scope) = parameters.scope.clone() {
        //     // Currently, the implementation requires all the scopes to be statically
        //     // defined in the server metadata. In the future, we might add support
        //     // for dynamic scopes.
        //     if !self
        //         .metadata
        //         .scopes_supported
        //         .clone()
        //         .unwrap()
        //         .contains(scope)
        //     {
        //         return Err(OAuthError::InvalidParametersError(
        //             "Scope is not supported by this server".to_string(),
        //         ));
        //     }
        // }
        //
        // if let Some(authorization_details) = parameters.authorization_details {
        //     //TODO
        //     return Err(OAuthError::InvalidAuthorizationDetailsError(
        //         "Unsupportedd authorization_details type".to_string(),
        //     ));
        // }
        //
        // // -----------------------
        // // Validate against client
        // // -----------------------
        //
        // let parameters = client.validate_request(parameters);
        //
        // // -------------------
        // // Validate parameters
        // // -------------------
        //
        // let redirect_uri = match parameters.redirect_uri {
        //     None => {
        //         // Should already be ensured by client.validateRequest(). Adding here for
        //         // clarity & extra safety.
        //         return Err(OAuthError::InvalidParametersError(
        //             "Missing reddirect_uri".to_string(),
        //         ));
        //     }
        //     Some(redirect_uri) => redirect_uri,
        // };
        //
        // // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-10#section-1.4.1
        // // > The authorization server MAY fully or partially ignore the scope
        // // > requested by the client, based on the authorization server policy or
        // // > the resource owner's instructions. If the issued access token scope is
        // // > different from the one requested by the client, the authorization
        // // > server MUST include the scope response parameter in the token response
        // // > (Section 3.2.3) to inform the client of the actual scope granted.
        //
        // // Let's make sure the scopes are unique (to reduce the token & storage size)
    }

    pub async fn get(
        &mut self,
        uri: OAuthRequestUri,
        client_id: ClientId,
        device_id: DeviceId,
    ) -> Result<RequestInfo, OAuthError> {
        unimplemented!()
        // let id: RequestId = decode_request_uri(uri.clone());
        // let request_data = match self.store.blocking_read().read_request(&id) {
        //     None => {
        //         return Err(OAuthError::InvalidRequestError(
        //             "Unknown request_uri".to_string(),
        //         ))
        //     }
        //     Some(request_data) => request_data.clone(),
        // };
        //
        // let mut updates = UpdateRequestData {
        //     ..Default::default()
        // };
        //
        // if request_data.sub.is_some() || request_data.code.is_some() {
        //     // If an account was linked to the request, the next step is to exchange
        //     // the code for a token.
        //     self.store.blocking_write().delete_request(id);
        //     return Err(OAuthError::AccessDeniedError(
        //         "This request was already authorized".to_string(),
        //     ));
        // }
        //
        // if request_data.expires_at < 1u32 {
        //     self.store.blocking_write().delete_request(id);
        //     return Err(OAuthError::AccessDeniedError(
        //         "This request has expired".to_string(),
        //     ));
        // } else {
        //     updates.expires_at = Some(1u32);
        // }
        //
        // if request_data.client_id != client_id.clone() {
        //     self.store.blocking_write().delete_request(id);
        //     return Err(OAuthError::AccessDeniedError(
        //         "This request was initiated for another client".to_string(),
        //     ));
        // }
        //
        // match request_data.device_id {
        //     None => {
        //         updates.device_id = Some(device_id);
        //     }
        //     Some(data_device_id) => {
        //         if data_device_id != device_id {
        //             self.store.blocking_write().delete_request(id);
        //             return Err(OAuthError::AccessDeniedError(
        //                 "This request was initiated for another device".to_string(),
        //             ));
        //         }
        //     }
        // }
        //
        // self.store
        //     .blocking_write()
        //     .update_request(id.clone(), updates.clone())?;
        //
        // RequestInfo {
        //     id,
        //     uri,
        //     expires_at: updates.expires_at.unwrap_or(request_data.expires_at),
        //     parameters: request_data.parameters.clone(),
        //     client_id: request_data.client_id.clone(),
        //     client_auth: request_data.client_auth.clone(),
        // }
    }

    pub async fn set_authorized(
        &mut self,
        uri: OAuthRequestUri,
        device_id: DeviceId,
        account: Account,
    ) -> Result<Code, OAuthError> {
        let id = decode_request_uri(uri);

        let data = match self.store.blocking_read().read_request(&id) {
            None => {
                return Err(OAuthError::InvalidRequestError(
                    "Unknown request uri".to_string(),
                ))
            }
            Some(data) => data.clone(),
        };

        if data.expires_at < 1u32 {
            self.store.blocking_write().delete_request(id);
            return Err(OAuthError::AccessDeniedError(
                "This request has expired".to_string(),
            ));
        }

        let data_device_id = match &data.device_id {
            None => {
                self.store.blocking_write().delete_request(id);
                return Err(OAuthError::AccessDeniedError(
                    "This request was not initiated".to_string(),
                ));
            }
            Some(device_id) => device_id.clone(),
        };

        if data_device_id != device_id {
            self.store.blocking_write().delete_request(id);
            return Err(OAuthError::AccessDeniedError(
                "This request was initiated from another device".to_string(),
            ));
        }

        if data.sub.is_some() || data.code.is_some() {
            self.store.blocking_write().delete_request(id);
            return Err(OAuthError::AccessDeniedError(
                "This request was already authorized".to_string(),
            ));
        }

        // Only response_type=code is supported
        let code = generate_code().await;

        // Bind the request to the account, preventing it from being used again.
        let update_request_data = UpdateRequestData {
            sub: Some(account.sub),
            code: Some(code.clone()),
            // Allow the client to exchange the code for a token within the next 60 seconds.
            expires_at: Some(AUTHORIZATION_INACTIVITY_TIMEOUT as u32),
            ..Default::default()
        };
        self.store
            .blocking_write()
            .update_request(id, update_request_data)?;

        Ok(code)
    }

    /**
     * @note If this method throws an error, any token previously generated from
     * the same `code` **must** me revoked.
     */
    pub async fn find_code(
        &mut self,
        client: Client,
        client_auth: ClientAuth,
        code: Code,
    ) -> Result<RequestDataAuthorized, OAuthError> {
        unimplemented!()
        // let request = match self.store.blocking_read().find_request_by_code(code) {
        //     None => return Err(OAuthError::InvalidGrantError("Invalid code".to_string())),
        //     Some(request) => request,
        // };
        //
        // let data = request.data;
        //
        // if !is_request_data_authorized(data.clone()) {
        //     // Should never happen: maybe the store implementation is faulty ?
        //     self.store.blocking_write().delete_request(request.id);
        //     return Err(OAuthError::RuntimeError(
        //         "Unexpected request state".to_string(),
        //     ));
        // }
        //
        // if data.client_id != client.id {
        //     // Note: do not reveal the original client ID to the client using an invalid id
        //     self.store.blocking_write().delete_request(request.id);
        //     return Err(OAuthError::InvalidGrantError(
        //         "The code was not issued to client".to_string(),
        //     ));
        // }
        //
        // if data.expires_at < 1u32 {
        //     self.store.blocking_write().delete_request(request.id);
        //     return Err(OAuthError::InvalidGrantError(
        //         "This code has expired".to_string(),
        //     ));
        // }
        //
        // if data.client_auth.method == "none" {
        //     // If the client did not use PAR, it was not authenticated when the
        //     // request was created (see authorize() method above). Since PAR is not
        //     // mandatory, and since the token exchange currently taking place *is*
        //     // authenticated (`clientAuth`), we allow "upgrading" the authentication
        //     // method (the token created will be bound to the current clientAuth).
        // } else {
        //     if client_auth.method != data.client_auth.method {
        //         self.store.blocking_write().delete_request(request.id);
        //         return Err(OAuthError::InvalidGrantError(
        //             "Invalid client authentication".to_string(),
        //         ));
        //     }
        //
        //     if !client.validate_client_auth(data.client_auth).await {
        //         self.store.blocking_write().delete_request(request.id);
        //         return Err(OAuthError::InvalidGrantError(
        //             "Invalid client authentication".to_string(),
        //         ));
        //     }
        // }
        //
        // Ok(RequestDataAuthorized {
        //     client_id: data.client_id,
        //     client_auth: data.client_auth,
        //     parameters: data.parameters,
        //     expires_at: data.expires_at,
        //     device_id: data.device_id,
        //     sub: data.sub,
        //     code: data.code,
        // })
    }

    pub async fn delete(&mut self, request_uri: &OAuthRequestUri) {
        unimplemented!()
        // let id = request_uri.request_id();
        // self.store.blocking_write().delete_request(id);
    }
}
