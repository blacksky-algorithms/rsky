use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::constants::{AUTHORIZATION_INACTIVITY_TIMEOUT, PAR_EXPIRES_IN};
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oauth_hooks::OAuthHooks;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::request::request_data::{RequestData, RequestDataAuthorized};
use crate::oauth_provider::request::request_id::{generate_request_id, RequestId};
use crate::oauth_provider::request::request_info::RequestInfo;
use crate::oauth_provider::request::request_store::{RequestStore, UpdateRequestData};
use crate::oauth_provider::request::request_uri::{
    decode_request_uri, encode_request_uri, RequestUri,
};
use crate::oauth_provider::signer::signer::Signer;
use crate::oauth_types::{
    OAuthAuthorizationRequestParameters, OAuthAuthorizationServerMetadata, OAuthClientId,
    OAuthCodeChallengeMethod, OAuthGrantType, OAuthResponseType, Prompt,
    CLIENT_ASSERTION_TYPE_JWT_BEARER,
};
use rocket::form::validate::Contains;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

pub struct RequestManager {
    store: Arc<RwLock<dyn RequestStore>>,
    signer: Arc<RwLock<Signer>>,
    metadata: OAuthAuthorizationServerMetadata,
    token_max_age: u64,
    hooks: OAuthHooks,
}

pub type RequestManagerCreator = Box<
    dyn Fn(Arc<RwLock<dyn RequestStore>>, Arc<RwLock<Signer>>, OAuthHooks) -> RequestManager
        + Send
        + Sync,
>;

impl RequestManager {
    pub fn creator(
        metadata: OAuthAuthorizationServerMetadata,
        token_max_age: u64,
    ) -> RequestManagerCreator {
        Box::new(
            move |store: Arc<RwLock<dyn RequestStore>>,
                  signer: Arc<RwLock<Signer>>,
                  hooks: OAuthHooks|
                  -> RequestManager {
                RequestManager::new(store, signer, metadata.clone(), token_max_age, hooks)
            },
        )
    }

    pub fn new(
        store: Arc<RwLock<dyn RequestStore>>,
        signer: Arc<RwLock<Signer>>,
        metadata: OAuthAuthorizationServerMetadata,
        token_max_age: u64,
        hooks: OAuthHooks,
    ) -> Self {
        RequestManager {
            store,
            signer,
            metadata,
            token_max_age,
            hooks,
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
        let expires_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros() as u64
            + PAR_EXPIRES_IN;
        let id = generate_request_id().await;

        let data = RequestData {
            client_id: client.id.clone(),
            client_auth: client_auth.clone(),
            parameters: parameters.clone(),
            expires_at,
            device_id,
            sub: None,
            code: None,
        };
        self.store.blocking_write().create_request(id.clone(), data);

        let uri = encode_request_uri(id.clone());

        Ok(RequestInfo {
            id,
            uri,
            parameters: parameters.clone(),
            expires_at,
            client_id: client.id.clone(),
            client_auth: client_auth.clone(),
        })
    }

    async fn validate(
        &self,
        client: Client,
        client_auth: ClientAuth,
        parameters: OAuthAuthorizationRequestParameters,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthAuthorizationRequestParameters, OAuthError> {
        // -------------------------------
        // Validate unsupported parameters
        // -------------------------------

        // Known unsupported OIDC parameters
        if parameters.claims.is_some() {
            return Err(OAuthError::InvalidParametersError(
                parameters,
                "Unsupported \"claims\" parameter".to_string(),
            ));
        }
        if parameters.id_token_hint.is_some() {
            return Err(OAuthError::InvalidParametersError(
                parameters,
                "Unsupported \"id_token_hint\" parameter".to_string(),
            ));
        }
        if parameters.nonce.is_some() {
            return Err(OAuthError::InvalidParametersError(
                parameters,
                "Unsupported \"nonce\" parameter".to_string(),
            ));
        }

        // -----------------------
        // Validate against server
        // -----------------------
        if let Some(response_types_supported) = &self.metadata.response_types_supported {
            if !response_types_supported.contains(parameters.response_type.as_str().to_string()) {
                return Err(OAuthError::AccessDeniedError(
                    "Unsupported response_type".to_string(),
                ));
            }
        }

        if let Some(grant_types_supported) = &self.metadata.grant_types_supported {
            if parameters.response_type == OAuthResponseType::Code
                && !grant_types_supported.contains(OAuthGrantType::AuthorizationCode)
            {
                return Err(OAuthError::AccessDeniedError(
                    "Unsupported grant_type \"authorization_code\"".to_string(),
                ));
            }
        }

        if let Some(scope) = parameters.scope.clone() {
            // Currently, the implementation requires all the scopes to be statically
            // defined in the server metadata. In the future, we might add support
            // for dynamic scopes.
            if let Some(scopes_supported) = &self.metadata.scopes_supported {
                if !scopes_supported.contains(scope.as_ref().to_string()) {
                    return Err(OAuthError::InvalidParametersError(
                        parameters,
                        "Scope is not supported by this server".to_string(),
                    ));
                }
            }
        }

        if let Some(authorization_details) = parameters.authorization_details.clone() {
            if let Some(details_types_supported) =
                &self.metadata.authorization_details_types_supported
            {
                for detail in authorization_details {
                    if !details_types_supported.contains(detail.type_().to_string()) {
                        return Err(OAuthError::InvalidAuthorizationDetailsError(
                            "Unsupportedd authorization_details type".to_string(),
                        ));
                    }
                }
            }
        }

        // -----------------------
        // Validate against client
        // -----------------------

        let parameters = client.validate_request(parameters)?;

        // -------------------
        // Validate parameters
        // -------------------

        let redirect_uri = match parameters.redirect_uri.clone() {
            None => {
                // Should already be ensured by client.validateRequest(). Adding here for
                // clarity & extra safety.
                return Err(OAuthError::InvalidParametersError(
                    parameters,
                    "Missing redirect_uri".to_string(),
                ));
            }
            Some(redirect_uri) => redirect_uri,
        };

        // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-10#section-1.4.1
        // > The authorization server MAY fully or partially ignore the scope
        // > requested by the client, based on the authorization server policy or
        // > the resource owner's instructions. If the issued access token scope is
        // > different from the one requested by the client, the authorization
        // > server MUST include the scope response parameter in the token response
        // > (Section 3.2.3) to inform the client of the actual scope granted.

        // Let's make sure the scopes are unique (to reduce the token & storage size)
        let scopes = match parameters.scope.clone() {
            None => {
                vec![]
            }
            Some(scope) => scope.iter().map(|val| val.to_string()).collect(),
        };
        let mut parameters = parameters.clone();

        // https://datatracker.ietf.org/doc/html/rfc9449#section-10
        if let Some(parameters_dpop_jkt) = parameters.dpop_jkt.clone() {
            if let Some(dpop_jkt) = dpop_jkt {
                if parameters_dpop_jkt != dpop_jkt {
                    return Err(OAuthError::InvalidParametersError(
                        parameters,
                        "\"dpop_jkt\" parameters does not match the DPoP proof".to_string(),
                    ));
                }
            } else {
                return Err(OAuthError::InvalidParametersError(
                    parameters,
                    "\"dpop_jkt\" parameters does not match the DPoP proof".to_string(),
                ));
            }
        } else {
            if let Some(dpop_jkt) = dpop_jkt {
                parameters.dpop_jkt = Some(dpop_jkt);
            }
        }

        if client_auth.method == CLIENT_ASSERTION_TYPE_JWT_BEARER {
            if let Some(dpop_jkt) = parameters.dpop_jkt.clone() {
                if client_auth.jkt == dpop_jkt {
                    return Err(OAuthError::InvalidParametersError(
                        parameters,
                        "The DPoP proof must be signed with a different key than the client assertion".to_string(),
                    ));
                }
            }
        }

        if parameters.code_challenge.is_some() {
            if parameters.code_challenge_method.is_none() {
                // https://datatracker.ietf.org/doc/html/rfc7636#section-4.3
                parameters.code_challenge_method = Some(OAuthCodeChallengeMethod::Plain);
            }
        } else {
            if parameters.code_challenge_method.is_some() {
                // https://datatracker.ietf.org/doc/html/rfc7636#section-4.4.1
                return Err(OAuthError::InvalidParametersError(
                    parameters,
                    "code_challenge is required when code_challenge_method is provided".to_string(),
                ));
            }

            // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-11#section-4.1.2.1
            //
            // > An AS MUST reject requests without a code_challenge from public
            // > clients, and MUST reject such requests from other clients unless
            // > there is reasonable assurance that the client mitigates
            // > authorization code injection in other ways. See Section 7.5.1 for
            // > details.
            //
            // > [...] In the specific deployment and the specific request, there is
            // > reasonable assurance by the authorization server that the client
            // > implements the OpenID Connect nonce mechanism properly.
            //
            // atproto does not implement the OpenID Connect nonce mechanism, so we
            // require the use of PKCE for all clients.
            return Err(OAuthError::InvalidParametersError(
                parameters,
                "Use of PKCE is required".to_string(),
            ));
        }

        // -----------------
        // atproto extension
        // -----------------
        if parameters.response_type != OAuthResponseType::Code {
            return Err(OAuthError::InvalidParametersError(
                parameters,
                "atproto only supports the \"code\" response_type".to_string(),
            ));
        }

        if !scopes.contains("atproto".to_string()) {
            return Err(OAuthError::InvalidScopeError(
                parameters,
                "The \"atproto\" scope is required".to_string(),
            ));
        } else if scopes.contains("openid".to_string()) {
            return Err(OAuthError::InvalidScopeError(
                parameters,
                "OpenID Connect is not compatible with atproto".to_string(),
            ));
        }

        if parameters.code_challenge_method.unwrap() != OAuthCodeChallengeMethod::S256 {
            return Err(OAuthError::InvalidScopeError(
                parameters,
                "atproto requires use of \"S256\" code_challenge_method".to_string(),
            ));
        }

        // atproto extension: if the client is not trusted, and not authenticated,
        // force users to consent to authorization requests. We do this to avoid
        // unauthenticated clients from being able to silently re-authenticate
        // users.
        if !client.info.is_trusted && !client.info.is_first_party && client_auth.method == "none" {
            if let Some(prompt) = parameters.prompt {
                match prompt {
                    Prompt::None => {
                        return Err(OAuthError::ConsentRequiredError(
                            parameters,
                            "Public clients are not allowed to use silent-sign-on".to_string(),
                        ));
                    }
                    _ => {
                        // force "consent" for unauthenticated, third party clients
                        parameters.prompt = Some(Prompt::Consent);
                    }
                }
            }
        }

        Ok(parameters)
    }

    pub async fn get(
        &mut self,
        uri: RequestUri,
        client_id: OAuthClientId,
        device_id: DeviceId,
    ) -> Result<RequestInfo, OAuthError> {
        let id: RequestId = decode_request_uri(&uri);
        let request_data = match self.store.blocking_read().read_request(&id) {
            None => {
                return Err(OAuthError::InvalidRequestError(
                    "Unknown request_uri".to_string(),
                ))
            }
            Some(request_data) => request_data.clone(),
        };

        let mut updates = UpdateRequestData {
            ..Default::default()
        };

        if request_data.sub.is_some() || request_data.code.is_some() {
            // If an account was linked to the request, the next step is to exchange
            // the code for a token.
            self.store.blocking_write().delete_request(id);
            return Err(OAuthError::AccessDeniedError(
                "This request was already authorized".to_string(),
            ));
        }

        if request_data.expires_at < 1u64 {
            self.store.blocking_write().delete_request(id);
            return Err(OAuthError::AccessDeniedError(
                "This request has expired".to_string(),
            ));
        } else {
            updates.expires_at = Some(1u64);
        }

        if request_data.client_id != client_id.clone() {
            self.store.blocking_write().delete_request(id);
            return Err(OAuthError::AccessDeniedError(
                "This request was initiated for another client".to_string(),
            ));
        }

        match request_data.device_id {
            None => {
                updates.device_id = Some(device_id);
            }
            Some(data_device_id) => {
                if data_device_id != device_id {
                    self.store.blocking_write().delete_request(id);
                    return Err(OAuthError::AccessDeniedError(
                        "This request was initiated for another device".to_string(),
                    ));
                }
            }
        }

        self.store
            .blocking_write()
            .update_request(id.clone(), updates.clone())?;

        Ok(RequestInfo {
            id,
            uri,
            expires_at: updates.expires_at.unwrap_or(request_data.expires_at),
            parameters: request_data.parameters.clone(),
            client_id: request_data.client_id.clone(),
            client_auth: request_data.client_auth.clone(),
        })
    }

    pub async fn set_authorized(
        &mut self,
        uri: &RequestUri,
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

        if data.expires_at < 1u64 {
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
        let code = Code::generate_code().await;

        // Bind the request to the account, preventing it from being used again.
        let update_request_data = UpdateRequestData {
            sub: Some(account.sub),
            code: Some(code.clone()),
            // Allow the client to exchange the code for a token within the next 60 seconds.
            expires_at: Some(AUTHORIZATION_INACTIVITY_TIMEOUT),
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
        let request = match self.store.blocking_read().find_request_by_code(code) {
            None => return Err(OAuthError::InvalidGrantError("Invalid code".to_string())),
            Some(request) => request,
        };

        let data = request.data;
        let authorized_request = match RequestDataAuthorized::new(data) {
            Ok(data) => data,
            Err(e) => {
                // Should never happen: maybe the store implementation is faulty ?
                self.store.blocking_write().delete_request(request.id);
                return Err(OAuthError::RuntimeError(
                    "Unexpected request state".to_string(),
                ));
            }
        };

        if authorized_request.client_id != client.id {
            // Note: do not reveal the original client ID to the client using an invalid id
            self.store.blocking_write().delete_request(request.id);
            return Err(OAuthError::InvalidGrantError(
                "The code was not issued to client".to_string(),
            ));
        }

        if authorized_request.expires_at < 1u64 {
            self.store.blocking_write().delete_request(request.id);
            return Err(OAuthError::InvalidGrantError(
                "This code has expired".to_string(),
            ));
        }

        if authorized_request.client_auth.method == "none" {
            // If the client did not use PAR, it was not authenticated when the
            // request was created (see authorize() method above). Since PAR is not
            // mandatory, and since the token exchange currently taking place *is*
            // authenticated (`clientAuth`), we allow "upgrading" the authentication
            // method (the token created will be bound to the current clientAuth).
        } else {
            if client_auth.method != authorized_request.client_auth.method {
                self.store.blocking_write().delete_request(request.id);
                return Err(OAuthError::InvalidGrantError(
                    "Invalid client authentication".to_string(),
                ));
            }

            if !client
                .validate_client_auth(&authorized_request.client_auth)
                .await
            {
                self.store.blocking_write().delete_request(request.id);
                return Err(OAuthError::InvalidGrantError(
                    "Invalid client authentication".to_string(),
                ));
            }
        }

        Ok(authorized_request)
    }

    pub async fn delete(&mut self, request_uri: &RequestUri) {
        let id = decode_request_uri(request_uri);
        self.store.blocking_write().delete_request(id);
    }
}
