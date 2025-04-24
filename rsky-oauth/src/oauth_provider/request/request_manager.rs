use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::constants::{AUTHORIZATION_INACTIVITY_TIMEOUT, PAR_EXPIRES_IN};
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::oauth_hooks::OAuthHooks;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::request::request_data::{RequestData, RequestDataAuthorized};
use crate::oauth_provider::request::request_id::RequestId;
use crate::oauth_provider::request::request_info::RequestInfo;
use crate::oauth_provider::request::request_store::{RequestStore, UpdateRequestData};
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_provider::signer::signer::Signer;
use crate::oauth_types::{
    OAuthAuthorizationRequestParameters, OAuthAuthorizationServerMetadata, OAuthClientId,
    OAuthCodeChallengeMethod, OAuthGrantType, OAuthResponseType, Prompt,
    CLIENT_ASSERTION_TYPE_JWT_BEARER,
};
use chrono::{DateTime, Utc};
use rocket::form::validate::Contains;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

pub struct RequestManager {
    store: Arc<RwLock<dyn RequestStore>>,
    signer: Arc<RwLock<Signer>>,
    metadata: OAuthAuthorizationServerMetadata,
    token_max_age: i64,
    hooks: Arc<OAuthHooks>,
}

impl RequestManager {
    pub fn new(
        store: Arc<RwLock<dyn RequestStore>>,
        signer: Arc<RwLock<Signer>>,
        metadata: OAuthAuthorizationServerMetadata,
        token_max_age: i64,
        hooks: Arc<OAuthHooks>,
    ) -> Self {
        RequestManager {
            store,
            signer,
            metadata,
            token_max_age,
            hooks,
        }
    }

    fn create_token_expiry(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_millis() as u64;
        now - self.token_max_age as u64
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

    async fn create(
        &mut self,
        client: Client,
        client_auth: ClientAuth,
        parameters: OAuthAuthorizationRequestParameters,
        device_id: Option<DeviceId>,
    ) -> Result<RequestInfo, OAuthError> {
        let expires_at = DateTime::from_timestamp(now_as_secs() + PAR_EXPIRES_IN, 0).unwrap();
        let id = RequestId::generate();

        let data = RequestData {
            client_id: client.id.clone(),
            client_auth: client_auth.clone(),
            parameters: parameters.clone(),
            expires_at,
            device_id,
            sub: None,
            code: None,
        };
        let mut store = self.store.write().await;
        store.create_request(id.clone(), data).await?;

        let uri = RequestUri::encode(id.clone());

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
                    parameters,
                    "Unsupported response_type".to_string(),
                    None,
                ));
            }
        }

        if let Some(grant_types_supported) = &self.metadata.grant_types_supported {
            if parameters.response_type == OAuthResponseType::Code
                && !grant_types_supported.contains(OAuthGrantType::AuthorizationCode)
            {
                return Err(OAuthError::AccessDeniedError(
                    parameters,
                    "Unsupported grant_type \"authorization_code\"".to_string(),
                    None,
                ));
            }
        }

        if let Some(scope) = parameters.scope.clone() {
            for scope in scope.iter() {
                // Currently, the implementation requires all the scopes to be statically
                // defined in the server metadata. In the future, we might add support
                // for dynamic scopes.
                if let Some(scopes_supported) = &self.metadata.scopes_supported {
                    if !scopes_supported.contains(scope.to_string()) {
                        return Err(OAuthError::InvalidParametersError(
                            parameters,
                            "Scope is not supported by this sserver".to_string(),
                        ));
                    }
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
                            parameters,
                            "Unsupported authorization_details type".to_string(),
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

        if let ClientAuth::Some(details) = client_auth.clone() {
            if let Some(dpop_jkt) = parameters.dpop_jkt.clone() {
                if details.jkt == dpop_jkt {
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
        if !client.info.is_trusted && !client.info.is_first_party && client_auth.is_none() {
            if let Some(prompt) = parameters.prompt {
                match prompt {
                    Prompt::None => {
                        return Err(OAuthError::ConsentRequiredError(
                            parameters,
                            Some(
                                "Public clients are not allowed to use silent-sign-on".to_string(),
                            ),
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
        let id: RequestId = RequestUri::decode(&uri);
        let store = self.store.read().await;
        let request_data = match store.read_request(&id).await {
            Ok(result) => match result {
                None => {
                    return Err(OAuthError::InvalidRequestError(
                        "Unknown request_uri".to_string(),
                    ))
                }
                Some(request_data) => request_data.clone(),
            },
            Err(_) => {
                return Err(OAuthError::InvalidRequestError(
                    "Unknown request_uri".to_string(),
                ))
            }
        };
        drop(store);

        let mut updates = UpdateRequestData {
            ..Default::default()
        };

        let mut store = self.store.write().await;
        if request_data.sub.is_some() || request_data.code.is_some() {
            // If an account was linked to the request, the next step is to exchange
            // the code for a token.
            store.delete_request(id).await?;
            return Err(OAuthError::AccessDeniedError(
                request_data.parameters,
                "This request was already authorized".to_string(),
                None,
            ));
        }

        let now = Utc::now().timestamp();
        if request_data.expires_at.timestamp() < now {
            store.delete_request(id).await?;
            return Err(OAuthError::AccessDeniedError(
                request_data.parameters,
                "This request has expired".to_string(),
                None,
            ));
        } else {
            updates.expires_at =
                Some(DateTime::from_timestamp(now + AUTHORIZATION_INACTIVITY_TIMEOUT, 0).unwrap());
        }

        if request_data.client_id != client_id.clone() {
            store.delete_request(id).await?;
            return Err(OAuthError::AccessDeniedError(
                request_data.parameters,
                "This request was initiated for another client".to_string(),
                None,
            ));
        }

        match request_data.device_id {
            None => {
                updates.device_id = Some(device_id);
            }
            Some(data_device_id) => {
                if data_device_id != device_id {
                    store.delete_request(id).await?;
                    return Err(OAuthError::AccessDeniedError(
                        request_data.parameters,
                        "This request was initiated for another device".to_string(),
                        None,
                    ));
                }
            }
        }
        store.update_request(id.clone(), updates.clone()).await?;

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
        device_id: &DeviceId,
        account: &Account,
    ) -> Result<Code, OAuthError> {
        let id = RequestUri::decode(uri);

        let store = self.store.read().await;
        let data = match store.read_request(&id).await {
            Ok(result) => match result {
                None => {
                    return Err(OAuthError::InvalidRequestError(
                        "Unknown request uri".to_string(),
                    ))
                }
                Some(data) => data.clone(),
            },
            Err(_) => {
                return Err(OAuthError::InvalidRequestError(
                    "Unknown request uri".to_string(),
                ))
            }
        };
        drop(store);

        if data.expires_at.timestamp() < Utc::now().timestamp() {
            let mut store = self.store.write().await;
            store.delete_request(id).await?;
            return Err(OAuthError::AccessDeniedError(
                data.parameters,
                "This request has expired".to_string(),
                None,
            ));
        }

        let data_device_id = match &data.device_id {
            None => {
                let mut store = self.store.write().await;
                store.delete_request(id).await?;
                return Err(OAuthError::AccessDeniedError(
                    data.parameters,
                    "This request was not initiated".to_string(),
                    None,
                ));
            }
            Some(device_id) => device_id.clone(),
        };

        if &data_device_id != device_id {
            let mut store = self.store.write().await;
            store.delete_request(id).await?;
            return Err(OAuthError::AccessDeniedError(
                data.parameters,
                "This request was initiated from another device".to_string(),
                None,
            ));
        }

        if data.sub.is_some() || data.code.is_some() {
            let mut store = self.store.write().await;
            store.delete_request(id).await?;
            return Err(OAuthError::AccessDeniedError(
                data.parameters,
                "This request was already authorized".to_string(),
                None,
            ));
        }

        // Only response_type=code is supported
        let code = Code::generate();

        // Bind the request to the account, preventing it from being used again.
        let update_request_data = UpdateRequestData {
            sub: Some(account.sub.clone()),
            code: Some(code.clone()),
            // Allow the client to exchange the code for a token within the next 60 seconds.
            expires_at: Some(
                DateTime::from_timestamp(now_as_secs() + AUTHORIZATION_INACTIVITY_TIMEOUT, 0)
                    .unwrap(),
            ),
            ..Default::default()
        };
        let mut store = self.store.write().await;
        store.update_request(id, update_request_data).await?;

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
        let store = self.store.read().await;
        let request = match store.find_request_by_code(code).await {
            None => return Err(OAuthError::InvalidGrantError("Invalid code".to_string())),
            Some(result) => result,
        };
        drop(store);

        let data = request.data;
        let authorized_request = match RequestDataAuthorized::new(data) {
            Ok(data) => data,
            Err(e) => {
                // Should never happen: maybe the store implementation is faulty ?
                let mut store = self.store.write().await;
                store.delete_request(request.id).await?;
                return Err(OAuthError::RuntimeError(
                    "Unexpected request state".to_string(),
                ));
            }
        };

        if authorized_request.client_id != client.id {
            // Note: do not reveal the original client ID to the client using an invalid id
            let mut store = self.store.write().await;
            store.delete_request(request.id).await?;
            return Err(OAuthError::InvalidGrantError(
                "The code was not issued to client".to_string(),
            ));
        }

        if authorized_request.expires_at.timestamp() < now_as_secs() {
            let mut store = self.store.write().await;
            store.delete_request(request.id).await?;
            return Err(OAuthError::InvalidGrantError(
                "This code has expired".to_string(),
            ));
        }

        if authorized_request.client_auth.is_none() {
            // If the client did not use PAR, it was not authenticated when the
            // request was created (see authorize() method above). Since PAR is not
            // mandatory, and since the token exchange currently taking place *is*
            // authenticated (`clientAuth`), we allow "upgrading" the authentication
            // method (the token created will be bound to the current clientAuth).
        } else {
            if client_auth.method() != authorized_request.client_auth.method() {
                let mut store = self.store.write().await;
                store.delete_request(request.id).await?;
                return Err(OAuthError::InvalidGrantError(
                    "Invalid client authentication".to_string(),
                ));
            }

            if !client
                .validate_client_auth(&authorized_request.client_auth)
                .await
            {
                let mut store = self.store.write().await;
                store.delete_request(request.id).await?;
                return Err(OAuthError::InvalidGrantError(
                    "Invalid client authentication".to_string(),
                ));
            }
        }

        Ok(authorized_request)
    }

    pub async fn delete(&mut self, request_uri: &RequestUri) {
        let id = RequestUri::decode(request_uri);
        let mut store = self.store.write().await;
        store.delete_request(id).await.unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::{Keyset, SignedJwt};
    use crate::jwk_jose::jose_key::JoseKey;
    use crate::oauth_provider::access_token::access_token_type::AccessTokenType;
    use crate::oauth_provider::client::client_auth::ClientAuthDetails;
    use crate::oauth_provider::client::client_info::ClientInfo;
    use crate::oauth_provider::constants::TOKEN_MAX_AGE;
    use crate::oauth_provider::request::request_store::FoundRequestResult;
    use crate::oauth_provider::token::token_id::{TokenId, TokenIdError};
    use crate::oauth_types::{
        ApplicationType, Display, OAuthClientMetadata, OAuthEndpointAuthMethod,
        OAuthIssuerIdentifier, OAuthRedirectUri, OAuthScope, ResponseMode, ValidUri, WebUri,
    };
    use biscuit::jwa::Algorithm;
    use biscuit::jwk::{AlgorithmParameters, CommonParameters, JWKSet, RSAKeyParameters, JWK};
    use biscuit::{jwa, Empty};
    use num_bigint::BigUint;
    use std::future::Future;
    use std::pin::Pin;

    struct TestRequestStore {}

    impl RequestStore for TestRequestStore {
        fn create_request(
            &mut self,
            id: RequestId,
            data: RequestData,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            Box::pin(async move { return Ok(()) })
        }

        fn read_request(
            &self,
            id: &RequestId,
        ) -> Pin<Box<dyn Future<Output = Result<Option<RequestData>, OAuthError>> + Send + Sync + '_>>
        {
            unimplemented!()
        }

        fn update_request(
            &mut self,
            id: RequestId,
            data: UpdateRequestData,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn delete_request(
            &mut self,
            id: RequestId,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn find_request_by_code(
            &self,
            code: Code,
        ) -> Pin<Box<dyn Future<Output = Option<FoundRequestResult>> + Send + Sync + '_>> {
            unimplemented!()
        }
    }

    async fn create_signer() -> Signer {
        let jwk = JWK {
            common: CommonParameters {
                algorithm: Some(Algorithm::Signature(jwa::SignatureAlgorithm::RS256)),
                key_id: Some("2011-04-29".to_string()),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                n: BigUint::new(vec![
                    2661337731, 446995658, 1209332140, 183172752, 955894533, 3140848734, 581365968,
                    3217299938, 3520742369, 1559833632, 1548159735, 2303031139, 1726816051,
                    92775838, 37272772, 1817499268, 2876656510, 1328166076, 2779910671, 4258539214,
                    2834014041, 3172137349, 4008354576, 121660540, 1941402830, 1620936445,
                    993798294, 47616683, 272681116, 983097263, 225284287, 3494334405, 4005126248,
                    1126447551, 2189379704, 4098746126, 3730484719, 3232696701, 2583545877,
                    428738419, 2533069420, 2922211325, 2227907999, 4154608099, 679827337,
                    1165541732, 2407118218, 3485541440, 799756961, 1854157941, 3062830172,
                    3270332715, 1431293619, 3068067851, 2238478449, 2704523019, 2826966453,
                    1548381401, 3719104923, 2605577849, 2293389158, 273345423, 169765991,
                    3539762026,
                ]),
                e: BigUint::new(vec![65537]),
                ..Default::default()
            }),
            additional: Default::default(),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
        let keyset = Keyset::new(vec![Box::new(jose_key)]);
        let keyset = Arc::new(RwLock::new(keyset));

        let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();

        Signer::new(issuer, keyset)
    }

    async fn create_request_manager() -> RequestManager {
        let store: Arc<RwLock<dyn RequestStore>> = Arc::new(RwLock::new(TestRequestStore {}));
        let signer: Arc<RwLock<Signer>> = Arc::new(RwLock::new(create_signer().await));
        let access_token_type = AccessTokenType::JWT;
        let max_age = Some(TOKEN_MAX_AGE);
        let oauth_hooks = OAuthHooks {
            on_client_info: Some(Box::new(
                |client_id: OAuthClientId,
                 oauth_client_metadata: OAuthClientMetadata,
                 jwks: Option<JWKSet<Empty>>|
                 -> ClientInfo {
                    ClientInfo {
                        is_first_party: client_id
                            == OAuthClientId::new(
                                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                            )
                            .unwrap(),
                        // @TODO make client client list configurable:
                        is_trusted: false,
                    }
                },
            )),
            on_authorization_details: None,
        };
        let metadata = OAuthAuthorizationServerMetadata::new(
            OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap(),
            WebUri::validate("https://pds.ripperoni.com/oauth/authorize").unwrap(),
            WebUri::validate("https://pds.ripperoni.com/oauth/token").unwrap(),
        );
        RequestManager::new(
            store,
            signer,
            metadata,
            max_age.unwrap(),
            Arc::new(oauth_hooks),
        )
    }

    #[test]
    fn test_create_token_expiry() {
        let token_id = TokenId::new("tok-7e415d9b2aec8f78d11d2b8c7144b87d").unwrap();
        assert_eq!(
            token_id.into_inner(),
            "tok-7e415d9b2aec8f78d11d2b8c7144b87d"
        );
        let token_id = TokenId::generate();
        let val = token_id.into_inner();
        TokenId::new(val).unwrap();

        let invalid_format_token_id =
            TokenId::new("abcd7e415d9b2aec8f78d11d2b8c7144b87d").unwrap_err();
        assert_eq!(invalid_format_token_id, TokenIdError::InvalidFormat);

        let invalid_length = TokenId::new("tok-dwadwda").unwrap_err();
        assert_eq!(invalid_length, TokenIdError::InvalidLength);
    }

    fn create_client_metadata() -> OAuthClientMetadata {
        OAuthClientMetadata {
            client_id: Some("https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string()),
            client_name: Some("cleanfollow-bsky".to_string()),
            client_uri: Some(WebUri::validate("https://cleanfollow-bsky.pages.dev").unwrap()),
            redirect_uris: vec![
                OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap()
            ],
            scope: Some(OAuthScope::new("atproto transition:generic").unwrap()),
            grant_types: vec![
                OAuthGrantType::AuthorizationCode,
                OAuthGrantType::RefreshToken,
            ],
            response_types: vec![OAuthResponseType::Code],
            dpop_bound_access_tokens: Some(true),
            token_endpoint_auth_method: Some(OAuthEndpointAuthMethod::None),
            application_type: ApplicationType::Web,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_create_request() {
        let mut request_manager = create_request_manager().await;
        let client = Client {
            id: OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json")
                .unwrap(),
            metadata: create_client_metadata(),
            jwks: None,
            info: ClientInfo {
                is_first_party: false,
                is_trusted: false,
            },
        };
        let client_auth = ClientAuth::new(Some(ClientAuthDetails {
            alg: "ES256".to_string(),
            kid: "dwadwadsda".to_string(),
            jkt: "placeholder".to_string(),
        }));
        let input = OAuthAuthorizationRequestParameters {
            client_id: OAuthClientId::new(
                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
            )
            .unwrap(),
            state: Some("vUbubHJLPlrwxXW-cRvB-A".to_string()),
            redirect_uri: Some(
                OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap(),
            ),
            scope: Some(OAuthScope::new("atproto transition:generic".to_string()).unwrap()),
            response_type: OAuthResponseType::Code,
            code_challenge: Some("GaWOuT8oEuWI2PNFDd2ejSaduXFOVF11ME6kA3tpnLM".to_string()),
            code_challenge_method: Some(OAuthCodeChallengeMethod::S256),
            dpop_jkt: None,
            response_mode: Some(ResponseMode::Fragment),
            nonce: None,
            max_age: None,
            claims: None,
            login_hint: Some("Ripperoni.com".to_string()),
            ui_locales: None,
            id_token_hint: None,
            display: Some(Display::Page),
            prompt: None,
            authorization_details: None,
        };
        let device_id: Option<DeviceId> = None;
        let dpop_jkt: Option<String> = None;
        let result = request_manager
            .create_authorization_request(
                client,
                client_auth.clone(),
                input,
                device_id,
                dpop_jkt.clone(),
            )
            .await
            .unwrap();
        let expected = RequestInfo {
            id: RequestId::new("req-f46e8a935aa5343574848e8a3c260fae").unwrap(),
            uri: RequestUri::new(
                "urn:ietf:params:oauth:request_uri:req-f46e8a935aa5343574848e8a3c260fae",
            )
            .unwrap(),
            parameters: OAuthAuthorizationRequestParameters {
                client_id: OAuthClientId::new(
                    "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                )
                .unwrap(),
                state: None,
                redirect_uri: None,
                scope: None,
                response_type: OAuthResponseType::Code,
                code_challenge: None,
                code_challenge_method: None,
                dpop_jkt,
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
            client_id: OAuthClientId::new(
                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
            )
            .unwrap(),
            client_auth,
        };
        assert_eq!(result, expected);
    }
}
