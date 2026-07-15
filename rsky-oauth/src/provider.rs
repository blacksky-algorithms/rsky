use crate::client::{Client, ClientManager, ClientMetadataFetcher, ParRequest};
use crate::dpop::{DpopManager, DpopProof, DpopRequest};
use crate::error::OAuthError;
use crate::jwk::{Jwk, JwkSet};
use crate::jwt;
use crate::jwt::{JwtClaims, JwtHeader};
use crate::request::{
    generate_code, generate_request_id, is_code, request_id_from_uri, request_uri_from_id,
    RequestData, AUTHORIZATION_INACTIVITY_TIMEOUT, PAR_EXPIRES_IN,
};
use crate::store::{AccountInfo, OAuthStore};
use crate::token::{
    generate_refresh_token, generate_token_id, is_refresh_token, is_token_id,
    verify_code_challenge, TokenData, TokenInfo, TOKEN_MAX_AGE,
};
use crate::types::*;
use serde_json::{json, Value};
use std::sync::Arc;
use url::Url;

pub const ACCESS_TOKEN_TYP: &str = "at+jwt";

/// Client identification material submitted with PAR/token/revoke calls.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClientCredentials {
    pub client_id: String,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
}

/// Response body of the PAR endpoint (RFC 9126 section 2.2).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParResponse {
    pub request_uri: String,
    pub expires_in: u64,
}

/// Body of a token endpoint request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
}

/// Everything the host application needs to render the authorization UI.
/// The provider never produces HTML.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizePageData {
    pub request_uri: String,
    pub client_id: String,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub client_trusted: bool,
    pub scopes: Vec<String>,
    pub login_hint: Option<String>,
    pub prompt: Option<String>,
    /// Accounts already signed in on this device.
    pub sessions: Vec<AccountInfo>,
}

/// A validated DPoP-bound access token presented to the resource server.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedAccess {
    pub did: String,
    pub scopes: Vec<String>,
    pub token_id: String,
}

pub struct OAuthProviderConfig {
    /// The AS issuer origin, e.g. `https://pds.example.com`.
    pub issuer: String,
    /// The `aud` of issued access tokens (the PDS service DID).
    pub audience: String,
    /// Private EC key used to sign access tokens.
    pub signing_key: Jwk,
    pub fetcher: Arc<dyn ClientMetadataFetcher>,
    pub store: Arc<dyn OAuthStore>,
    pub dpop: DpopManager,
    /// client_ids treated as trusted (first-party) for UI purposes.
    pub trusted_clients: Vec<String>,
}

pub struct OAuthProvider {
    issuer: String,
    audience: String,
    signing_key: Jwk,
    clients: ClientManager,
    store: Arc<dyn OAuthStore>,
    dpop: DpopManager,
    trusted_clients: Vec<String>,
}

impl OAuthProvider {
    pub fn new(config: OAuthProviderConfig) -> Self {
        Self {
            issuer: config.issuer,
            audience: config.audience,
            signing_key: config.signing_key,
            clients: ClientManager::new(config.fetcher),
            store: config.store,
            dpop: config.dpop,
            trusted_clients: config.trusted_clients,
        }
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn store(&self) -> &Arc<dyn OAuthStore> {
        &self.store
    }

    /// The value for the `DPoP-Nonce` response header.
    pub fn next_dpop_nonce(&self, now: u64) -> Option<String> {
        self.dpop.next_nonce(now)
    }

    async fn authenticated_client(
        &self,
        credentials: &ClientCredentials,
        now: u64,
    ) -> Result<(Client, ClientAuth), OAuthError> {
        if credentials.client_id.is_empty() {
            return Err(OAuthError::InvalidRequest(
                "client_id is required".to_string(),
            ));
        }
        let client = self.clients.get_client(&credentials.client_id).await?;
        let client_auth = client.authenticate(
            credentials.client_assertion_type.as_deref(),
            credentials.client_assertion.as_deref(),
            &self.issuer,
            now,
        )?;
        Ok((client, client_auth))
    }

    /// RFC 9126 pushed authorization request.
    pub async fn pushed_authorization_request(
        &self,
        credentials: &ClientCredentials,
        request: &ParRequest,
        dpop: &DpopRequest<'_>,
        now: u64,
    ) -> Result<ParResponse, OAuthError> {
        let (client, client_auth) = self.authenticated_client(credentials, now).await?;
        let proof = self.dpop.check_proof(dpop, now)?;
        let mut parameters = client.validate_request(request)?;
        parameters.dpop_jkt = proof.map(|proof| proof.jkt);
        let request_id = generate_request_id();
        let data = RequestData {
            client_id: client.id.clone(),
            client_auth,
            parameters,
            expires_at: now + PAR_EXPIRES_IN,
            device_id: None,
            did: None,
            code: None,
        };
        self.store.create_request(&request_id, &data).await?;
        Ok(ParResponse {
            request_uri: request_uri_from_id(&request_id),
            expires_in: PAR_EXPIRES_IN,
        })
    }

    /// Loads the pending request for the authorization flow, binding it
    /// to the device and sliding its inactivity window.
    async fn active_request(
        &self,
        client_id: &str,
        request_uri: &str,
        device_id: &str,
        now: u64,
    ) -> Result<(String, RequestData), OAuthError> {
        let request_id = request_id_from_uri(request_uri)?;
        let Some(mut data) = self.store.read_request(request_id).await? else {
            return Err(OAuthError::InvalidRequest(
                "unknown request_uri".to_string(),
            ));
        };
        if data.is_authorized() {
            self.store.delete_request(request_id).await?;
            return Err(OAuthError::InvalidGrant(
                "request was already authorized".to_string(),
            ));
        }
        if data.is_expired(now) {
            self.store.delete_request(request_id).await?;
            return Err(OAuthError::InvalidGrant(
                "this request has expired".to_string(),
            ));
        }
        if data.client_id != client_id {
            return Err(OAuthError::InvalidRequest(
                "client_id does not match the request".to_string(),
            ));
        }
        match &data.device_id {
            None => data.device_id = Some(device_id.to_string()),
            Some(bound) if bound != device_id => {
                self.store.delete_request(request_id).await?;
                return Err(OAuthError::InvalidGrant(
                    "request was initiated from another device".to_string(),
                ));
            }
            Some(_) => {}
        }
        data.expires_at = now + AUTHORIZATION_INACTIVITY_TIMEOUT;
        self.store.update_request(request_id, &data).await?;
        Ok((request_id.to_string(), data))
    }

    /// GET /oauth/authorize: returns the data the host renders.
    pub async fn authorize(
        &self,
        client_id: &str,
        request_uri: &str,
        device_id: &str,
        now: u64,
    ) -> Result<AuthorizePageData, OAuthError> {
        let (_, data) = self
            .active_request(client_id, request_uri, device_id, now)
            .await?;
        let client = self.clients.get_client(client_id).await?;
        let sessions = match &data.parameters.login_hint {
            Some(hint) => self
                .store
                .list_device_accounts(device_id)
                .await?
                .into_iter()
                .filter(|account| account_matches_hint(account, hint))
                .collect(),
            None => self.store.list_device_accounts(device_id).await?,
        };
        Ok(AuthorizePageData {
            request_uri: request_uri.to_string(),
            client_id: client.id.clone(),
            client_name: client.metadata.client_name.clone(),
            client_uri: client.metadata.client_uri.clone(),
            logo_uri: client.metadata.logo_uri.clone(),
            client_trusted: self.trusted_clients.contains(&client.id),
            scopes: data
                .parameters
                .scope
                .split_ascii_whitespace()
                .map(String::from)
                .collect(),
            login_hint: data.parameters.login_hint.clone(),
            prompt: data.parameters.prompt.clone(),
            sessions,
        })
    }

    /// POST sign-in during the authorization flow.
    pub async fn sign_in(
        &self,
        client_id: &str,
        request_uri: &str,
        device_id: &str,
        identifier: &str,
        password: &str,
        now: u64,
    ) -> Result<AccountInfo, OAuthError> {
        let (_, data) = self
            .active_request(client_id, request_uri, device_id, now)
            .await?;
        let Some(account) = self
            .store
            .authenticate_account(identifier, password)
            .await?
        else {
            return Err(OAuthError::InvalidRequest(
                "invalid identifier or password".to_string(),
            ));
        };
        if let Some(hint) = &data.parameters.login_hint {
            if !account_matches_hint(&account, hint) {
                return Err(OAuthError::InvalidRequest(
                    "account does not match the requested login_hint".to_string(),
                ));
            }
        }
        self.store
            .upsert_device_account(device_id, &account.did)
            .await?;
        Ok(account)
    }

    /// Consent accepted: issues the authorization code and returns the
    /// redirect URL for the client callback.
    pub async fn accept(
        &self,
        client_id: &str,
        request_uri: &str,
        device_id: &str,
        did: &str,
        now: u64,
    ) -> Result<String, OAuthError> {
        let (request_id, mut data) = self
            .active_request(client_id, request_uri, device_id, now)
            .await?;
        if self
            .store
            .get_device_account(device_id, did)
            .await?
            .is_none()
        {
            return Err(OAuthError::InvalidRequest(
                "account is not signed in on this device".to_string(),
            ));
        }
        let code = generate_code();
        data.did = Some(did.to_string());
        data.code = Some(code.clone());
        data.expires_at = now + AUTHORIZATION_INACTIVITY_TIMEOUT;
        self.store.update_request(&request_id, &data).await?;
        self.store
            .set_authorized_client(did, client_id, &data.parameters.scope)
            .await?;
        self.build_redirect(&data.parameters, &[("code", &code)])
    }

    /// Consent denied: discards the request and returns the error
    /// redirect URL.
    pub async fn reject(
        &self,
        client_id: &str,
        request_uri: &str,
        device_id: &str,
        now: u64,
    ) -> Result<String, OAuthError> {
        let (request_id, data) = self
            .active_request(client_id, request_uri, device_id, now)
            .await?;
        self.store.delete_request(&request_id).await?;
        self.build_redirect(
            &data.parameters,
            &[
                ("error", "access_denied"),
                ("error_description", "Access denied"),
            ],
        )
    }

    fn build_redirect(
        &self,
        parameters: &AuthorizationRequestParameters,
        pairs: &[(&str, &str)],
    ) -> Result<String, OAuthError> {
        let mut url = Url::parse(&parameters.redirect_uri)
            .map_err(|_| OAuthError::ServerError("stored redirect_uri is invalid".to_string()))?;
        {
            let mut query = url.query_pairs_mut();
            for (key, value) in pairs {
                query.append_pair(key, value);
            }
            if let Some(state) = &parameters.state {
                query.append_pair("state", state);
            }
            query.append_pair("iss", &self.issuer);
        }
        Ok(url.into())
    }

    /// POST /oauth/token.
    pub async fn token(
        &self,
        credentials: &ClientCredentials,
        request: &TokenRequest,
        dpop: &DpopRequest<'_>,
        now: u64,
    ) -> Result<TokenResponse, OAuthError> {
        let (client, client_auth) = self.authenticated_client(credentials, now).await?;
        let Some(proof) = self.dpop.check_proof(dpop, now)? else {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP proof is required".to_string(),
            ));
        };
        match request.grant_type.as_str() {
            GRANT_AUTHORIZATION_CODE => {
                self.authorization_code_grant(&client, client_auth, request, &proof, now)
                    .await
            }
            GRANT_REFRESH_TOKEN => {
                self.refresh_token_grant(&client, client_auth, request, &proof, now)
                    .await
            }
            other => Err(OAuthError::InvalidRequest(format!(
                "unsupported grant_type \"{other}\""
            ))),
        }
    }

    async fn authorization_code_grant(
        &self,
        client: &Client,
        client_auth: ClientAuth,
        request: &TokenRequest,
        proof: &DpopProof,
        now: u64,
    ) -> Result<TokenResponse, OAuthError> {
        let code = match request.code.as_deref() {
            Some(code) if is_code(code) => code,
            _ => return Err(OAuthError::InvalidGrant("invalid code".to_string())),
        };
        let Some((_, data)) = self.store.consume_request_code(code).await? else {
            // A missing code that matches an issued token is a replay:
            // revoke the token and sign the device out.
            if let Some(token) = self.store.find_token_by_code(code).await? {
                self.store.delete_token(&token.token_id).await?;
                if let Some(device_id) = &token.data.device_id {
                    self.store
                        .remove_device_account(device_id, &token.data.did)
                        .await?;
                }
            }
            return Err(OAuthError::InvalidGrant("invalid code".to_string()));
        };
        if data.is_expired(now) {
            return Err(OAuthError::InvalidGrant(
                "this code has expired".to_string(),
            ));
        }
        let Some(did) = &data.did else {
            return Err(OAuthError::InvalidGrant(
                "request was not authorized".to_string(),
            ));
        };
        if data.client_id != client.id {
            return Err(OAuthError::InvalidGrant(
                "code was issued to another client".to_string(),
            ));
        }
        compare_client_auth(&data.client_auth, &client_auth)?;
        let Some(code_verifier) = request.code_verifier.as_deref() else {
            return Err(OAuthError::InvalidGrant(
                "code_verifier is required".to_string(),
            ));
        };
        verify_code_challenge(code_verifier, &data.parameters.code_challenge)?;
        if let Some(redirect_uri) = request.redirect_uri.as_deref() {
            if redirect_uri != data.parameters.redirect_uri {
                return Err(OAuthError::InvalidGrant(
                    "redirect_uri does not match".to_string(),
                ));
            }
        }
        let jkt = match &data.parameters.dpop_jkt {
            Some(jkt) if *jkt != proof.jkt => {
                return Err(OAuthError::InvalidGrant(
                    "DPoP key does not match the key bound at PAR time".to_string(),
                ))
            }
            Some(jkt) => jkt.clone(),
            None => proof.jkt.clone(),
        };
        let Some(account) = self.store.get_account(did).await? else {
            return Err(OAuthError::InvalidGrant("account not found".to_string()));
        };
        let token_id = generate_token_id();
        let refresh_token = client
            .metadata
            .grant_types
            .iter()
            .any(|grant| grant == GRANT_REFRESH_TOKEN)
            .then(generate_refresh_token);
        let mut parameters = data.parameters.clone();
        parameters.dpop_jkt = Some(jkt.clone());
        let token_data = TokenData {
            created_at: now,
            updated_at: now,
            expires_at: now + TOKEN_MAX_AGE,
            client_id: client.id.clone(),
            client_auth,
            device_id: data.device_id.clone(),
            did: account.did.clone(),
            parameters: parameters.clone(),
            code: Some(code.to_string()),
        };
        self.store
            .create_token(&token_id, &token_data, refresh_token.as_deref())
            .await?;
        self.build_token_response(&token_id, &token_data, refresh_token, now)
    }

    async fn refresh_token_grant(
        &self,
        client: &Client,
        client_auth: ClientAuth,
        request: &TokenRequest,
        proof: &DpopProof,
        now: u64,
    ) -> Result<TokenResponse, OAuthError> {
        let refresh_token = match request.refresh_token.as_deref() {
            Some(refresh_token) if is_refresh_token(refresh_token) => refresh_token,
            _ => {
                return Err(OAuthError::InvalidGrant(
                    "invalid refresh token".to_string(),
                ))
            }
        };
        let Some(token) = self
            .store
            .find_token_by_refresh_token(refresh_token)
            .await?
        else {
            return Err(OAuthError::InvalidGrant(
                "invalid refresh token".to_string(),
            ));
        };
        if token.current_refresh_token.as_deref() != Some(refresh_token) {
            self.store.delete_token(&token.token_id).await?;
            return Err(OAuthError::InvalidGrant(
                "refresh token replayed".to_string(),
            ));
        }
        match self
            .validate_refresh(client, &client_auth, &token, proof, now)
            .await
        {
            Ok(response) => Ok(response),
            Err(err) => {
                // Any failure after presenting a valid current refresh
                // token revokes the session.
                self.store.delete_token(&token.token_id).await?;
                Err(err)
            }
        }
    }

    async fn validate_refresh(
        &self,
        client: &Client,
        client_auth: &ClientAuth,
        token: &TokenInfo,
        proof: &DpopProof,
        now: u64,
    ) -> Result<TokenResponse, OAuthError> {
        if token.data.client_id != client.id {
            return Err(OAuthError::InvalidGrant(
                "refresh token was issued to another client".to_string(),
            ));
        }
        if !client
            .metadata
            .grant_types
            .iter()
            .any(|grant| grant == GRANT_REFRESH_TOKEN)
        {
            return Err(OAuthError::InvalidGrant(
                "client metadata does not declare the refresh_token grant".to_string(),
            ));
        }
        compare_client_auth(&token.data.client_auth, client_auth)?;
        if token.data.parameters.dpop_jkt.as_deref() != Some(proof.jkt.as_str()) {
            return Err(OAuthError::InvalidGrant(
                "DPoP key does not match the session key".to_string(),
            ));
        }
        token.data.validate_refresh_lifetimes(now)?;
        let new_token_id = generate_token_id();
        let new_refresh_token = generate_refresh_token();
        self.store
            .rotate_token(
                &token.token_id,
                &new_token_id,
                &new_refresh_token,
                now,
                now + TOKEN_MAX_AGE,
            )
            .await?;
        let mut data = token.data.clone();
        data.updated_at = now;
        data.expires_at = now + TOKEN_MAX_AGE;
        self.build_token_response(&new_token_id, &data, Some(new_refresh_token), now)
    }

    fn build_token_response(
        &self,
        token_id: &str,
        data: &TokenData,
        refresh_token: Option<String>,
        now: u64,
    ) -> Result<TokenResponse, OAuthError> {
        let jkt = data
            .parameters
            .dpop_jkt
            .as_deref()
            .ok_or_else(|| OAuthError::ServerError("missing DPoP key binding".to_string()))?;
        let mut header = JwtHeader::new(self.signing_key.curve()?.alg());
        header.typ = Some(ACCESS_TOKEN_TYP.to_string());
        let mut claims = JwtClaims {
            iss: Some(self.issuer.clone()),
            sub: Some(data.did.clone()),
            aud: Some(Value::String(self.audience.clone())),
            exp: Some(data.expires_at),
            iat: Some(now),
            jti: Some(token_id.to_string()),
            ..Default::default()
        };
        claims
            .extra
            .insert("scope".to_string(), json!(data.parameters.scope));
        claims
            .extra
            .insert("client_id".to_string(), json!(data.client_id));
        claims.extra.insert("cnf".to_string(), json!({"jkt": jkt}));
        let access_token = jwt::sign(&header, &claims, &self.signing_key)?;
        Ok(TokenResponse {
            access_token,
            token_type: "DPoP".to_string(),
            expires_in: TOKEN_MAX_AGE,
            refresh_token,
            scope: data.parameters.scope.clone(),
            sub: data.did.clone(),
        })
    }

    /// RFC 7009 token revocation. Unknown tokens succeed silently.
    pub async fn revoke(
        &self,
        credentials: &ClientCredentials,
        token: &str,
        now: u64,
    ) -> Result<(), OAuthError> {
        let (client, _) = self.authenticated_client(credentials, now).await?;
        let found = if is_token_id(token) {
            self.store.read_token(token).await?
        } else if is_refresh_token(token) {
            self.store.find_token_by_refresh_token(token).await?
        } else if is_code(token) {
            self.store.find_token_by_code(token).await?
        } else {
            match jwt::decode(token) {
                Ok(decoded) => match decoded.claims.jti {
                    Some(jti) => self.store.read_token(&jti).await?,
                    None => None,
                },
                Err(_) => None,
            }
        };
        if let Some(token) = found {
            if token.data.client_id == client.id {
                self.store.delete_token(&token.token_id).await?;
            }
        }
        Ok(())
    }

    /// Validates a DPoP-bound access token presented to the resource
    /// server, including revocation via the store.
    pub async fn verify_access_token(
        &self,
        access_token: &str,
        dpop: &DpopRequest<'_>,
        now: u64,
    ) -> Result<VerifiedAccess, OAuthError> {
        let decoded = jwt::verify(access_token, &self.signing_key.to_public())?;
        decoded.header.validate_typ(ACCESS_TOKEN_TYP)?;
        decoded.claims.validate_time(now, jwt::DEFAULT_CLOCK_SKEW)?;
        decoded.claims.validate_iss(&self.issuer)?;
        decoded.claims.validate_aud(&self.audience)?;
        let token_id = match decoded.claims.jti.as_deref() {
            Some(jti) if is_token_id(jti) => jti.to_string(),
            _ => {
                return Err(OAuthError::InvalidToken(
                    "malformed access token".to_string(),
                ))
            }
        };
        let Some(did) = decoded.claims.sub.clone() else {
            return Err(OAuthError::InvalidToken(
                "malformed access token".to_string(),
            ));
        };
        let scope = decoded
            .claims
            .extra
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let scopes: Vec<String> = scope.split_ascii_whitespace().map(String::from).collect();
        if !scopes.iter().any(|scope| scope == SCOPE_ATPROTO) {
            return Err(OAuthError::InvalidToken(format!(
                "access token is missing the \"{SCOPE_ATPROTO}\" scope"
            )));
        }
        let Some(jkt) = decoded
            .claims
            .extra
            .get("cnf")
            .and_then(|cnf| cnf.get("jkt"))
            .and_then(Value::as_str)
        else {
            return Err(OAuthError::InvalidToken(
                "access token is not DPoP-bound".to_string(),
            ));
        };
        let Some(proof) = self.dpop.check_proof(dpop, now)? else {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP proof is required".to_string(),
            ));
        };
        if proof.jkt != jkt {
            return Err(OAuthError::InvalidToken(
                "DPoP key does not match the token binding".to_string(),
            ));
        }
        // Stateful check: honors revocation and rotation.
        let Some(stored) = self.store.read_token(&token_id).await? else {
            return Err(OAuthError::InvalidToken(
                "access token was revoked".to_string(),
            ));
        };
        if stored.data.did != did || stored.data.expires_at <= now {
            return Err(OAuthError::InvalidToken(
                "access token has expired".to_string(),
            ));
        }
        Ok(VerifiedAccess {
            did,
            scopes,
            token_id,
        })
    }

    /// The public JWK set served at /oauth/jwks.
    pub fn jwks(&self) -> JwkSet {
        JwkSet {
            keys: vec![self.signing_key.to_public()],
        }
    }

    /// RFC 8414 authorization server metadata document.
    pub fn authorization_server_metadata(&self) -> Value {
        let issuer = &self.issuer;
        json!({
            "issuer": issuer,
            "scopes_supported": [
                SCOPE_ATPROTO,
                SCOPE_TRANSITION_EMAIL,
                SCOPE_TRANSITION_GENERIC,
                SCOPE_TRANSITION_CHAT_BSKY,
            ],
            "subject_types_supported": ["public"],
            "response_types_supported": [RESPONSE_TYPE_CODE],
            "response_modes_supported": ["query"],
            "grant_types_supported": [GRANT_AUTHORIZATION_CODE, GRANT_REFRESH_TOKEN],
            "code_challenge_methods_supported": [CODE_CHALLENGE_METHOD_S256],
            "ui_locales_supported": ["en-US"],
            "display_values_supported": ["page", "popup", "touch"],
            "prompt_values_supported": ["consent", "create"],
            "authorization_response_iss_parameter_supported": true,
            "request_parameter_supported": false,
            "request_uri_parameter_supported": true,
            "require_request_uri_registration": true,
            "jwks_uri": format!("{issuer}/oauth/jwks"),
            "authorization_endpoint": format!("{issuer}/oauth/authorize"),
            "token_endpoint": format!("{issuer}/oauth/token"),
            "token_endpoint_auth_methods_supported": [
                AUTH_METHOD_NONE,
                AUTH_METHOD_PRIVATE_KEY_JWT,
            ],
            "token_endpoint_auth_signing_alg_values_supported": ["ES256", "ES256K"],
            "revocation_endpoint": format!("{issuer}/oauth/revoke"),
            "pushed_authorization_request_endpoint": format!("{issuer}/oauth/par"),
            "require_pushed_authorization_requests": true,
            "dpop_signing_alg_values_supported": ["ES256", "ES256K"],
            "client_id_metadata_document_supported": true,
            "protected_resources": [issuer],
        })
    }

    /// RFC 9728 protected resource metadata document.
    pub fn protected_resource_metadata(&self) -> Value {
        json!({
            "resource": self.issuer,
            "authorization_servers": [self.issuer],
            "scopes_supported": [],
            "bearer_methods_supported": ["header"],
            "resource_documentation": "https://atproto.com",
        })
    }
}

fn account_matches_hint(account: &AccountInfo, hint: &str) -> bool {
    account.did == hint
        || account
            .handle
            .as_deref()
            .is_some_and(|handle| handle.eq_ignore_ascii_case(hint))
}

fn compare_client_auth(original: &ClientAuth, current: &ClientAuth) -> Result<(), OAuthError> {
    let matches = match (original, current) {
        (ClientAuth::None, ClientAuth::None) => true,
        (
            ClientAuth::PrivateKeyJwt { jkt: original, .. },
            ClientAuth::PrivateKeyJwt { jkt: current, .. },
        ) => original == current,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(OAuthError::InvalidGrant(
            "client authentication does not match the initial request".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientMetadataFetcher;
    use crate::dpop::{DpopManager, DpopNonce, InMemoryReplayStore};
    use crate::jwk::EcCurve;
    use crate::store::MemoryOAuthStore;
    use crate::types::{
        AUTH_METHOD_PRIVATE_KEY_JWT, CLIENT_ASSERTION_TYPE_JWT_BEARER, CODE_CHALLENGE_METHOD_S256,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    const NOW: u64 = 1_700_000_000;
    const ISSUER: &str = "https://pds.example.com";
    const AUDIENCE: &str = "did:web:pds.example.com";
    const CLIENT_ID: &str = "https://app.example.com/oauth/client-metadata.json";
    const OTHER_CLIENT_ID: &str = "https://other.example.com/oauth/client-metadata.json";
    const DEVICE: &str = "dev-00000000000000000000000000000000";
    const PKCE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const PKCE_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    static JTI: AtomicU64 = AtomicU64::new(0);

    fn signing_key() -> Jwk {
        Jwk::from_private_key_bytes(EcCurve::K256, &[0x42u8; 32]).unwrap()
    }

    fn dpop_key() -> Jwk {
        Jwk::from_private_key_bytes(EcCurve::P256, &[0x51u8; 32]).unwrap()
    }

    fn other_dpop_key() -> Jwk {
        Jwk::from_private_key_bytes(EcCurve::P256, &[0x52u8; 32]).unwrap()
    }

    fn client_assertion_key() -> Jwk {
        let mut key = Jwk::from_private_key_bytes(EcCurve::P256, &[0x53u8; 32]).unwrap();
        key.kid = Some("key-1".to_string());
        key
    }

    fn public_metadata(client_id: &str) -> OAuthClientMetadata {
        let mut metadata = OAuthClientMetadata::new(client_id);
        let origin = url::Url::parse(client_id).unwrap();
        metadata.redirect_uris = vec![format!("https://{}/callback", origin.host_str().unwrap())];
        metadata.grant_types = vec![
            GRANT_AUTHORIZATION_CODE.to_string(),
            GRANT_REFRESH_TOKEN.to_string(),
        ];
        metadata.scope = Some("atproto transition:generic".to_string());
        metadata.dpop_bound_access_tokens = true;
        metadata.client_name = Some("Example App".to_string());
        metadata
    }

    fn confidential_metadata(client_id: &str) -> OAuthClientMetadata {
        let mut metadata = public_metadata(client_id);
        metadata.token_endpoint_auth_method = Some(AUTH_METHOD_PRIVATE_KEY_JWT.to_string());
        metadata.token_endpoint_auth_signing_alg = Some("ES256".to_string());
        metadata.jwks = Some(JwkSet {
            keys: vec![client_assertion_key().to_public()],
        });
        metadata
    }

    struct StubFetcher {
        clients: Vec<OAuthClientMetadata>,
    }

    #[async_trait::async_trait]
    impl ClientMetadataFetcher for StubFetcher {
        async fn fetch_client_metadata(
            &self,
            url: &str,
        ) -> Result<OAuthClientMetadata, OAuthError> {
            self.clients
                .iter()
                .find(|metadata| metadata.client_id == url)
                .cloned()
                .ok_or_else(|| OAuthError::InvalidClient("client metadata fetch failed".into()))
        }

        async fn fetch_jwks(&self, _url: &str) -> Result<JwkSet, OAuthError> {
            Err(OAuthError::InvalidClient("no jwks".into()))
        }
    }

    struct Setup {
        provider: OAuthProvider,
        store: Arc<MemoryOAuthStore>,
    }

    fn setup_with(clients: Vec<OAuthClientMetadata>, nonce: Option<DpopNonce>) -> Setup {
        let store = Arc::new(MemoryOAuthStore::new());
        store.add_account(
            AccountInfo {
                did: "did:plc:alice".to_string(),
                handle: Some("alice.example.com".to_string()),
                email: Some("alice@example.com".to_string()),
                deactivated: false,
            },
            "correct-password",
        );
        store.add_account(
            AccountInfo {
                did: "did:plc:bob".to_string(),
                handle: Some("bob.example.com".to_string()),
                email: None,
                deactivated: false,
            },
            "bobs-password",
        );
        let provider = OAuthProvider::new(OAuthProviderConfig {
            issuer: ISSUER.to_string(),
            audience: AUDIENCE.to_string(),
            signing_key: signing_key(),
            fetcher: Arc::new(StubFetcher { clients }),
            store: store.clone(),
            dpop: DpopManager::new(nonce, Box::new(InMemoryReplayStore::default())),
            trusted_clients: vec![],
        });
        Setup { provider, store }
    }

    fn setup() -> Setup {
        setup_with(
            vec![public_metadata(CLIENT_ID), public_metadata(OTHER_CLIENT_ID)],
            None,
        )
    }

    fn proof(
        key: &Jwk,
        htm: &str,
        htu: &str,
        now: u64,
        nonce: Option<&str>,
        ath: Option<&str>,
    ) -> String {
        let mut header = JwtHeader::new(key.curve().unwrap().alg());
        header.typ = Some("dpop+jwt".to_string());
        header.jwk = Some(key.to_public());
        let mut claims = JwtClaims {
            iat: Some(now),
            jti: Some(format!("jti-{}", JTI.fetch_add(1, Ordering::SeqCst))),
            ..Default::default()
        };
        claims.extra.insert("htm".to_string(), json!(htm));
        claims.extra.insert("htu".to_string(), json!(htu));
        if let Some(nonce) = nonce {
            claims.extra.insert("nonce".to_string(), json!(nonce));
        }
        if let Some(ath) = ath {
            use base64::engine::general_purpose::URL_SAFE_NO_PAD;
            use base64::Engine;
            use sha2::{Digest, Sha256};
            claims.extra.insert(
                "ath".to_string(),
                json!(URL_SAFE_NO_PAD.encode(Sha256::digest(ath.as_bytes()))),
            );
        }
        jwt::sign(&header, &claims, key).unwrap()
    }

    fn credentials(client_id: &str) -> ClientCredentials {
        ClientCredentials {
            client_id: client_id.to_string(),
            client_assertion_type: None,
            client_assertion: None,
        }
    }

    fn par_request(client_id: &str) -> ParRequest {
        let metadata = public_metadata(client_id);
        ParRequest {
            client_id: client_id.to_string(),
            response_type: "code".to_string(),
            redirect_uri: Some(metadata.redirect_uris[0].clone()),
            scope: Some("atproto transition:generic".to_string()),
            state: Some("state-123".to_string()),
            code_challenge: Some(PKCE_CHALLENGE.to_string()),
            code_challenge_method: Some(CODE_CHALLENGE_METHOD_S256.to_string()),
            login_hint: None,
            prompt: None,
        }
    }

    async fn run_par(setup: &Setup, client_id: &str, key: &Jwk, now: u64) -> ParResponse {
        let htu = format!("{ISSUER}/oauth/par");
        let proof = proof(key, "POST", &htu, now, None, None);
        let headers = [proof.as_str()];
        setup
            .provider
            .pushed_authorization_request(
                &credentials(client_id),
                &par_request(client_id),
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                now,
            )
            .await
            .unwrap()
    }

    /// PAR -> authorize -> sign_in -> accept, returning the code.
    async fn run_authorization(setup: &Setup, client_id: &str, key: &Jwk, now: u64) -> String {
        let par = run_par(setup, client_id, key, now).await;
        let page = setup
            .provider
            .authorize(client_id, &par.request_uri, DEVICE, now)
            .await
            .unwrap();
        assert_eq!(page.client_id, client_id);
        setup
            .provider
            .sign_in(
                client_id,
                &par.request_uri,
                DEVICE,
                "alice.example.com",
                "correct-password",
                now,
            )
            .await
            .unwrap();
        let redirect = setup
            .provider
            .accept(client_id, &par.request_uri, DEVICE, "did:plc:alice", now)
            .await
            .unwrap();
        let url = Url::parse(&redirect).unwrap();
        let code = url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())
            .unwrap();
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "state" && value == "state-123"));
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "iss" && value == ISSUER));
        code
    }

    async fn run_token(
        setup: &Setup,
        client_id: &str,
        key: &Jwk,
        code: &str,
        now: u64,
    ) -> Result<TokenResponse, OAuthError> {
        let htu = format!("{ISSUER}/oauth/token");
        let proof = proof(key, "POST", &htu, now, None, None);
        let headers = [proof.as_str()];
        setup
            .provider
            .token(
                &credentials(client_id),
                &TokenRequest {
                    grant_type: GRANT_AUTHORIZATION_CODE.to_string(),
                    code: Some(code.to_string()),
                    redirect_uri: None,
                    code_verifier: Some(PKCE_VERIFIER.to_string()),
                    refresh_token: None,
                },
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                now,
            )
            .await
    }

    async fn run_refresh(
        setup: &Setup,
        client_id: &str,
        key: &Jwk,
        refresh_token: &str,
        now: u64,
    ) -> Result<TokenResponse, OAuthError> {
        let htu = format!("{ISSUER}/oauth/token");
        let proof = proof(key, "POST", &htu, now, None, None);
        let headers = [proof.as_str()];
        setup
            .provider
            .token(
                &credentials(client_id),
                &TokenRequest {
                    grant_type: GRANT_REFRESH_TOKEN.to_string(),
                    refresh_token: Some(refresh_token.to_string()),
                    ..Default::default()
                },
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                now,
            )
            .await
    }

    async fn run_verify(
        setup: &Setup,
        key: &Jwk,
        access_token: &str,
        now: u64,
    ) -> Result<VerifiedAccess, OAuthError> {
        let htu = format!("{ISSUER}/xrpc/com.atproto.server.getSession");
        let proof = proof(key, "GET", &htu, now, None, Some(access_token));
        let headers = [proof.as_str()];
        setup
            .provider
            .verify_access_token(
                access_token,
                &DpopRequest {
                    method: "GET",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: Some(access_token),
                },
                now,
            )
            .await
    }

    #[tokio::test]
    async fn full_flow_public_client() {
        let setup = setup();
        let key = dpop_key();
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        assert_eq!(tokens.token_type, "DPoP");
        assert_eq!(tokens.expires_in, TOKEN_MAX_AGE);
        assert_eq!(tokens.sub, "did:plc:alice");
        assert_eq!(tokens.scope, "atproto transition:generic");
        let refresh_token = tokens.refresh_token.clone().unwrap();
        assert!(crate::token::is_refresh_token(&refresh_token));

        let access = run_verify(&setup, &key, &tokens.access_token, NOW + 10)
            .await
            .unwrap();
        assert_eq!(access.did, "did:plc:alice");
        assert_eq!(access.scopes, vec!["atproto", "transition:generic"]);

        // rotate
        let rotated = run_refresh(&setup, CLIENT_ID, &key, &refresh_token, NOW + 100)
            .await
            .unwrap();
        assert_ne!(rotated.access_token, tokens.access_token);
        let new_refresh = rotated.refresh_token.clone().unwrap();
        assert_ne!(new_refresh, refresh_token);

        // the pre-rotation access token is no longer recognized
        let err = run_verify(&setup, &key, &tokens.access_token, NOW + 110)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("revoked"));
        run_verify(&setup, &key, &rotated.access_token, NOW + 110)
            .await
            .unwrap();

        // replaying the rotated-out refresh token kills the session
        let err = run_refresh(&setup, CLIENT_ID, &key, &refresh_token, NOW + 120)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("replayed"));
        let err = run_verify(&setup, &key, &rotated.access_token, NOW + 130)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("revoked"));
    }

    #[tokio::test]
    async fn full_flow_confidential_client() {
        let setup = setup_with(vec![confidential_metadata(CLIENT_ID)], None);
        let key = dpop_key();
        let assertion_key = client_assertion_key();
        let make_credentials = |now: u64| {
            let mut header = JwtHeader::new("ES256");
            header.kid = assertion_key.kid.clone();
            let claims = JwtClaims {
                iss: Some(CLIENT_ID.to_string()),
                sub: Some(CLIENT_ID.to_string()),
                aud: Some(json!(ISSUER)),
                iat: Some(now),
                exp: Some(now + 60),
                jti: Some(format!("assert-{}", JTI.fetch_add(1, Ordering::SeqCst))),
                ..Default::default()
            };
            ClientCredentials {
                client_id: CLIENT_ID.to_string(),
                client_assertion_type: Some(CLIENT_ASSERTION_TYPE_JWT_BEARER.to_string()),
                client_assertion: Some(jwt::sign(&header, &claims, &assertion_key).unwrap()),
            }
        };

        let htu = format!("{ISSUER}/oauth/par");
        let par_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [par_proof.as_str()];
        let par = setup
            .provider
            .pushed_authorization_request(
                &make_credentials(NOW),
                &par_request(CLIENT_ID),
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap();
        setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap();
        setup
            .provider
            .sign_in(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "did:plc:alice",
                "correct-password",
                NOW,
            )
            .await
            .unwrap();
        let redirect = setup
            .provider
            .accept(CLIENT_ID, &par.request_uri, DEVICE, "did:plc:alice", NOW)
            .await
            .unwrap();
        let url = Url::parse(&redirect).unwrap();
        let code = url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())
            .unwrap();

        let token_htu = format!("{ISSUER}/oauth/token");
        let token_proof = proof(&key, "POST", &token_htu, NOW, None, None);
        let token_headers = [token_proof.as_str()];
        let tokens = setup
            .provider
            .token(
                &make_credentials(NOW),
                &TokenRequest {
                    grant_type: GRANT_AUTHORIZATION_CODE.to_string(),
                    code: Some(code),
                    redirect_uri: Some("https://app.example.com/callback".to_string()),
                    code_verifier: Some(PKCE_VERIFIER.to_string()),
                    refresh_token: None,
                },
                &DpopRequest {
                    method: "POST",
                    uri: &token_htu,
                    dpop_headers: &token_headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap();

        // refresh with the same client key succeeds
        let refresh_proof = proof(&key, "POST", &token_htu, NOW + 50, None, None);
        let refresh_headers = [refresh_proof.as_str()];
        setup
            .provider
            .token(
                &make_credentials(NOW + 50),
                &TokenRequest {
                    grant_type: GRANT_REFRESH_TOKEN.to_string(),
                    refresh_token: tokens.refresh_token.clone(),
                    ..Default::default()
                },
                &DpopRequest {
                    method: "POST",
                    uri: &token_htu,
                    dpop_headers: &refresh_headers,
                    access_token: None,
                },
                NOW + 50,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn loopback_client_flow() {
        let setup = setup_with(vec![], None);
        let key = dpop_key();
        let client_id = "http://localhost";
        let htu = format!("{ISSUER}/oauth/par");
        let par_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [par_proof.as_str()];
        let mut request = par_request(client_id);
        request.scope = Some("atproto".to_string());
        // port wildcard on the default registered loopback redirect
        request.redirect_uri = Some("http://127.0.0.1:49152/".to_string());
        let par = setup
            .provider
            .pushed_authorization_request(
                &credentials(client_id),
                &request,
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap();
        assert_eq!(par.expires_in, 300);
        assert!(par
            .request_uri
            .starts_with("urn:ietf:params:oauth:request_uri:req-"));
        let page = setup
            .provider
            .authorize(client_id, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap();
        assert_eq!(page.scopes, vec!["atproto"]);
        assert!(!page.client_trusted);
    }

    #[tokio::test]
    async fn par_rejects_bad_clients_and_scopes() {
        let setup = setup();
        let key = dpop_key();
        let htu = format!("{ISSUER}/oauth/par");

        // empty client_id
        let par_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [par_proof.as_str()];
        let err = setup
            .provider
            .pushed_authorization_request(
                &credentials(""),
                &par_request(CLIENT_ID),
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("client_id is required"));

        // unresolvable client
        let par_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [par_proof.as_str()];
        let err = setup
            .provider
            .pushed_authorization_request(
                &credentials("https://unknown.example.com/client.json"),
                &par_request("https://unknown.example.com/client.json"),
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("fetch failed"));

        // scope not registered
        let par_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [par_proof.as_str()];
        let mut request = par_request(CLIENT_ID);
        request.scope = Some("atproto transition:chat.bsky".to_string());
        let err = setup
            .provider
            .pushed_authorization_request(
                &credentials(CLIENT_ID),
                &request,
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("not registered"));
    }

    #[tokio::test]
    async fn dpop_nonce_dance() {
        let nonce = DpopNonce::new([7u8; 32], 60).unwrap();
        let expected = nonce.next(NOW);
        let setup = setup_with(vec![public_metadata(CLIENT_ID)], Some(nonce));
        let key = dpop_key();
        let htu = format!("{ISSUER}/oauth/par");

        // no nonce: rejected with use_dpop_nonce and a fresh nonce offered
        let par_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [par_proof.as_str()];
        let err = setup
            .provider
            .pushed_authorization_request(
                &credentials(CLIENT_ID),
                &par_request(CLIENT_ID),
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.requires_dpop_nonce());
        assert_eq!(setup.provider.next_dpop_nonce(NOW), Some(expected.clone()));

        // retry with the provided nonce
        let par_proof = proof(&key, "POST", &htu, NOW, Some(&expected), None);
        let headers = [par_proof.as_str()];
        setup
            .provider
            .pushed_authorization_request(
                &credentials(CLIENT_ID),
                &par_request(CLIENT_ID),
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn authorize_request_lifecycle_failures() {
        let setup = setup();
        let key = dpop_key();

        // unknown request uri
        let err = setup
            .provider
            .authorize(
                CLIENT_ID,
                "urn:ietf:params:oauth:request_uri:req-00000000000000000000000000000000",
                DEVICE,
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("unknown request_uri"));

        // malformed request uri
        let err = setup
            .provider
            .authorize(CLIENT_ID, "urn:nope", DEVICE, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("invalid request_uri"));

        // expired
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let err = setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW + 301)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("expired"));

        // wrong client
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let err = setup
            .provider
            .authorize(OTHER_CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("does not match"));

        // device binding
        setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap();
        let err = setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, "dev-other", NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("another device"));

        // already authorized
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        assert!(crate::request::is_code(&code));
        let par = {
            // recover the request uri from the store: accept marked it authorized
            // so a second authorize on it must fail; recreate a full round instead
            run_par(&setup, CLIENT_ID, &key, NOW).await
        };
        setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap();
        setup
            .provider
            .sign_in(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "alice.example.com",
                "correct-password",
                NOW,
            )
            .await
            .unwrap();
        setup
            .provider
            .accept(CLIENT_ID, &par.request_uri, DEVICE, "did:plc:alice", NOW)
            .await
            .unwrap();
        let err = setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("already authorized"));
    }

    #[tokio::test]
    async fn sign_in_failures_and_login_hint() {
        let setup = setup();
        let key = dpop_key();
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let err = setup
            .provider
            .sign_in(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "alice.example.com",
                "wrong-password",
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err
            .error_description()
            .contains("invalid identifier or password"));

        // login_hint restricts both the page sessions and sign-in
        let htu = format!("{ISSUER}/oauth/par");
        let par_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [par_proof.as_str()];
        let mut request = par_request(CLIENT_ID);
        request.login_hint = Some("alice.example.com".to_string());
        let par = setup
            .provider
            .pushed_authorization_request(
                &credentials(CLIENT_ID),
                &request,
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap();
        let err = setup
            .provider
            .sign_in(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "bob.example.com",
                "bobs-password",
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("login_hint"));
        setup
            .provider
            .sign_in(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "alice.example.com",
                "correct-password",
                NOW,
            )
            .await
            .unwrap();
        // hint-filtered sessions on the page
        let page = setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap();
        assert_eq!(page.sessions.len(), 1);
        assert_eq!(page.login_hint.as_deref(), Some("alice.example.com"));
    }

    #[tokio::test]
    async fn accept_requires_signed_in_account() {
        let setup = setup();
        let key = dpop_key();
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let err = setup
            .provider
            .accept(CLIENT_ID, &par.request_uri, DEVICE, "did:plc:alice", NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("not signed in"));
    }

    #[tokio::test]
    async fn reject_redirects_with_access_denied() {
        let setup = setup();
        let key = dpop_key();
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let redirect = setup
            .provider
            .reject(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap();
        let url = Url::parse(&redirect).unwrap();
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "error" && value == "access_denied"));
        assert!(url.query_pairs().any(|(key, _)| key == "iss"));
        // the request is gone
        let err = setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("unknown request_uri"));
    }

    #[tokio::test]
    async fn token_grant_failures() {
        let setup = setup();
        let key = dpop_key();
        let htu = format!("{ISSUER}/oauth/token");

        // unsupported grant type
        let token_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [token_proof.as_str()];
        let err = setup
            .provider
            .token(
                &credentials(CLIENT_ID),
                &TokenRequest {
                    grant_type: "password".to_string(),
                    ..Default::default()
                },
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("unsupported grant_type"));

        // missing DPoP proof
        let err = setup
            .provider
            .token(
                &credentials(CLIENT_ID),
                &TokenRequest {
                    grant_type: GRANT_AUTHORIZATION_CODE.to_string(),
                    ..Default::default()
                },
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &[],
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("DPoP proof is required"));

        // malformed and unknown codes
        for code in [
            "not-a-code",
            "cod-0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            let err = run_token(&setup, CLIENT_ID, &key, code, NOW)
                .await
                .unwrap_err();
            assert!(err.error_description().contains("invalid code"));
        }
    }

    #[tokio::test]
    async fn token_code_validation_failures() {
        let setup = setup();
        let key = dpop_key();

        // expired code
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let err = run_token(&setup, CLIENT_ID, &key, &code, NOW + 301)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("expired"));

        // wrong client
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let err = run_token(&setup, OTHER_CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("another client"));

        // bad PKCE verifier
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let htu = format!("{ISSUER}/oauth/token");
        let token_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [token_proof.as_str()];
        let err = setup
            .provider
            .token(
                &credentials(CLIENT_ID),
                &TokenRequest {
                    grant_type: GRANT_AUTHORIZATION_CODE.to_string(),
                    code: Some(code.clone()),
                    code_verifier: Some("wrong-verifier-wrong-verifier-wrong-verifier".to_string()),
                    ..Default::default()
                },
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("code_verifier"));

        // missing verifier
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let token_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [token_proof.as_str()];
        let err = setup
            .provider
            .token(
                &credentials(CLIENT_ID),
                &TokenRequest {
                    grant_type: GRANT_AUTHORIZATION_CODE.to_string(),
                    code: Some(code.clone()),
                    code_verifier: None,
                    ..Default::default()
                },
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err
            .error_description()
            .contains("code_verifier is required"));

        // redirect mismatch
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let token_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [token_proof.as_str()];
        let err = setup
            .provider
            .token(
                &credentials(CLIENT_ID),
                &TokenRequest {
                    grant_type: GRANT_AUTHORIZATION_CODE.to_string(),
                    code: Some(code.clone()),
                    code_verifier: Some(PKCE_VERIFIER.to_string()),
                    redirect_uri: Some("https://app.example.com/other".to_string()),
                    ..Default::default()
                },
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err
            .error_description()
            .contains("redirect_uri does not match"));

        // DPoP key mismatch with the PAR-bound key
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let err = run_token(&setup, CLIENT_ID, &other_dpop_key(), &code, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("bound at PAR time"));
    }

    #[tokio::test]
    async fn code_replay_revokes_token_and_device_session() {
        let setup = setup();
        let key = dpop_key();
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        run_verify(&setup, &key, &tokens.access_token, NOW)
            .await
            .unwrap();

        // replaying the code revokes the token and signs the device out
        let err = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("invalid code"));
        let err = run_verify(&setup, &key, &tokens.access_token, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("revoked"));
        assert!(setup
            .store
            .get_device_account(DEVICE, "did:plc:alice")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn refresh_grant_failures() {
        let setup = setup();
        let key = dpop_key();

        for refresh in [
            "garbage",
            "ref-0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            let err = run_refresh(&setup, CLIENT_ID, &key, refresh, NOW)
                .await
                .unwrap_err();
            assert!(err.error_description().contains("invalid refresh token"));
        }

        // issued to another client
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        let refresh = tokens.refresh_token.clone().unwrap();
        let err = run_refresh(&setup, OTHER_CLIENT_ID, &key, &refresh, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("another client"));
        // ... and that failure revoked the session
        let err = run_refresh(&setup, CLIENT_ID, &key, &refresh, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("invalid refresh token"));

        // DPoP key mismatch
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        let refresh = tokens.refresh_token.clone().unwrap();
        let err = run_refresh(&setup, CLIENT_ID, &other_dpop_key(), &refresh, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("session key"));

        // lifetime exhaustion (public client: 2 weeks)
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        let refresh = tokens.refresh_token.clone().unwrap();
        let err = run_refresh(
            &setup,
            CLIENT_ID,
            &key,
            &refresh,
            NOW + crate::token::PUBLIC_CLIENT_SESSION_LIFETIME + 1,
        )
        .await
        .unwrap_err();
        assert!(err.error_description().contains("expired"));
    }

    #[tokio::test]
    async fn revoke_supports_every_token_form() {
        let setup = setup();
        let key = dpop_key();

        // by refresh token
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        setup
            .provider
            .revoke(
                &credentials(CLIENT_ID),
                tokens.refresh_token.as_deref().unwrap(),
                NOW,
            )
            .await
            .unwrap();
        assert!(run_verify(&setup, &key, &tokens.access_token, NOW)
            .await
            .is_err());

        // by access token JWT
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        setup
            .provider
            .revoke(&credentials(CLIENT_ID), &tokens.access_token, NOW)
            .await
            .unwrap();
        assert!(run_verify(&setup, &key, &tokens.access_token, NOW)
            .await
            .is_err());

        // by code
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        setup
            .provider
            .revoke(&credentials(CLIENT_ID), &code, NOW)
            .await
            .unwrap();
        assert!(run_verify(&setup, &key, &tokens.access_token, NOW)
            .await
            .is_err());

        // wrong client: silently ignored, token survives
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        setup
            .provider
            .revoke(&credentials(OTHER_CLIENT_ID), &tokens.access_token, NOW)
            .await
            .unwrap();
        run_verify(&setup, &key, &tokens.access_token, NOW)
            .await
            .unwrap();

        // unknown / garbage tokens are silent successes
        setup
            .provider
            .revoke(
                &credentials(CLIENT_ID),
                "tok-00000000000000000000000000000000",
                NOW,
            )
            .await
            .unwrap();
        setup
            .provider
            .revoke(&credentials(CLIENT_ID), "complete-garbage", NOW)
            .await
            .unwrap();
        let no_jti = jwt::sign(
            &JwtHeader::new("ES256K"),
            &JwtClaims::default(),
            &signing_key(),
        )
        .unwrap();
        setup
            .provider
            .revoke(&credentials(CLIENT_ID), &no_jti, NOW)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn verify_access_token_failures() {
        let setup = setup();
        let key = dpop_key();
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();

        // missing proof
        let err = setup
            .provider
            .verify_access_token(
                &tokens.access_token,
                &DpopRequest {
                    method: "GET",
                    uri: &format!("{ISSUER}/xrpc/x"),
                    dpop_headers: &[],
                    access_token: Some(&tokens.access_token),
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("DPoP proof is required"));

        // proof signed by a different key than the token binding
        let err = run_verify(&setup, &other_dpop_key(), &tokens.access_token, NOW)
            .await
            .unwrap_err();
        assert!(err
            .error_description()
            .contains("does not match the token binding"));

        // garbage token
        assert!(run_verify(&setup, &key, "garbage", NOW).await.is_err());

        // expired access token (still stored, exp in the past)
        let err = run_verify(&setup, &key, &tokens.access_token, NOW + TOKEN_MAX_AGE + 61)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("expired"));

        // hand-minted tokens exercising each malformed-claims branch
        type TokenMutation = Box<dyn Fn(&mut JwtHeader, &mut JwtClaims)>;
        let mint = |mutate: TokenMutation| {
            let mut header = JwtHeader::new("ES256K");
            header.typ = Some(ACCESS_TOKEN_TYP.to_string());
            let mut claims = JwtClaims {
                iss: Some(ISSUER.to_string()),
                sub: Some("did:plc:alice".to_string()),
                aud: Some(json!(AUDIENCE)),
                exp: Some(NOW + 100),
                iat: Some(NOW),
                jti: Some("tok-00000000000000000000000000000000".to_string()),
                ..Default::default()
            };
            claims.extra.insert("scope".to_string(), json!("atproto"));
            claims.extra.insert(
                "cnf".to_string(),
                json!({"jkt": dpop_key().to_public().thumbprint()}),
            );
            mutate(&mut header, &mut claims);
            jwt::sign(&header, &claims, &signing_key()).unwrap()
        };

        let cases: Vec<(TokenMutation, &str)> = vec![
            (
                Box::new(|header: &mut JwtHeader, _: &mut JwtClaims| {
                    header.typ = Some("JWT".to_string())
                }),
                "unexpected JWT \"typ\"",
            ),
            (
                Box::new(|_: &mut JwtHeader, claims: &mut JwtClaims| {
                    claims.jti = Some("not-a-token-id".to_string())
                }),
                "malformed access token",
            ),
            (
                Box::new(|_: &mut JwtHeader, claims: &mut JwtClaims| claims.jti = None),
                "malformed access token",
            ),
            (
                Box::new(|_: &mut JwtHeader, claims: &mut JwtClaims| claims.sub = None),
                "malformed access token",
            ),
            (
                Box::new(|_: &mut JwtHeader, claims: &mut JwtClaims| {
                    claims
                        .extra
                        .insert("scope".to_string(), json!("transition:generic"));
                }),
                "missing the \"atproto\" scope",
            ),
            (
                Box::new(|_: &mut JwtHeader, claims: &mut JwtClaims| {
                    claims.extra.remove("cnf");
                }),
                "not DPoP-bound",
            ),
            (
                Box::new(|_: &mut JwtHeader, claims: &mut JwtClaims| {
                    claims.iss = Some("https://evil.example.com".to_string())
                }),
                "iss",
            ),
        ];
        for (mutate, fragment) in cases {
            let token = mint(mutate);
            let err = run_verify(&setup, &key, &token, NOW).await.unwrap_err();
            let desc = err.error_description().to_string();
            assert!(desc.contains(fragment), "expected {fragment:?} in {desc:?}");
        }

        // valid claims but no stored token: revoked
        let token = mint(Box::new(|_, _| {}));
        let err = run_verify(&setup, &key, &token, NOW).await.unwrap_err();
        assert!(err.error_description().contains("revoked"));
    }

    #[tokio::test]
    async fn store_state_edge_cases() {
        let setup = setup();
        let key = dpop_key();

        // a code bound to a request that was never authorized
        let code = crate::request::generate_code();
        let data = RequestData {
            client_id: CLIENT_ID.to_string(),
            client_auth: ClientAuth::None,
            parameters: AuthorizationRequestParameters {
                client_id: CLIENT_ID.to_string(),
                response_type: "code".to_string(),
                redirect_uri: "https://app.example.com/callback".to_string(),
                scope: "atproto".to_string(),
                state: None,
                code_challenge: PKCE_CHALLENGE.to_string(),
                code_challenge_method: CODE_CHALLENGE_METHOD_S256.to_string(),
                login_hint: None,
                prompt: None,
                dpop_jkt: None,
            },
            expires_at: NOW + 300,
            device_id: Some(DEVICE.to_string()),
            did: None,
            code: Some(code.clone()),
        };
        setup.store.create_request("req-x", &data).await.unwrap();
        let err = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("not authorized"));

        // a request whose client auth no longer matches
        let code = crate::request::generate_code();
        let mut data = data.clone();
        data.did = Some("did:plc:alice".to_string());
        data.code = Some(code.clone());
        data.client_auth = ClientAuth::PrivateKeyJwt {
            alg: "ES256".to_string(),
            kid: "key-1".to_string(),
            jkt: "thumb".to_string(),
        };
        setup.store.create_request("req-y", &data).await.unwrap();
        let err = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("client authentication"));

        // an authorized request for an account that no longer exists;
        // its PAR dpop binding is absent so the proof key is adopted
        let code = crate::request::generate_code();
        let mut data = data.clone();
        data.client_auth = ClientAuth::None;
        data.did = Some("did:plc:ghost".to_string());
        data.code = Some(code.clone());
        setup.store.create_request("req-z", &data).await.unwrap();
        let err = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("account not found"));

        // a stored token whose client metadata lost the refresh grant
        let mut no_refresh = public_metadata(CLIENT_ID);
        no_refresh.grant_types = vec![GRANT_AUTHORIZATION_CODE.to_string()];
        let restricted = setup_with(vec![no_refresh], None);
        let refresh_token = crate::token::generate_refresh_token();
        let token_data = TokenData {
            created_at: NOW,
            updated_at: NOW,
            expires_at: NOW + TOKEN_MAX_AGE,
            client_id: CLIENT_ID.to_string(),
            client_auth: ClientAuth::None,
            device_id: None,
            did: "did:plc:alice".to_string(),
            parameters: data.parameters.clone(),
            code: None,
        };
        restricted
            .store
            .create_token("tok-restricted", &token_data, Some(&refresh_token))
            .await
            .unwrap();
        let err = run_refresh(&restricted, CLIENT_ID, &key, &refresh_token, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("refresh_token grant"));

        // a stored token whose did was orphaned but sub mismatch on verify
        let jkt = key.to_public().thumbprint();
        let mut bound = token_data.clone();
        bound.parameters.dpop_jkt = Some(jkt);
        setup
            .store
            .create_token("tok-00000000000000000000000000000000", &bound, None)
            .await
            .unwrap();
        let mut header = JwtHeader::new("ES256K");
        header.typ = Some(ACCESS_TOKEN_TYP.to_string());
        let mut claims = JwtClaims {
            iss: Some(ISSUER.to_string()),
            sub: Some("did:plc:mallory".to_string()),
            aud: Some(json!(AUDIENCE)),
            exp: Some(NOW + 100),
            iat: Some(NOW),
            jti: Some("tok-00000000000000000000000000000000".to_string()),
            ..Default::default()
        };
        claims.extra.insert("scope".to_string(), json!("atproto"));
        claims.extra.insert(
            "cnf".to_string(),
            json!({"jkt": key.to_public().thumbprint()}),
        );
        let token = jwt::sign(&header, &claims, &signing_key()).unwrap();
        let err = run_verify(&setup, &key, &token, NOW).await.unwrap_err();
        assert!(err.error_description().contains("expired"));
    }

    #[tokio::test]
    async fn code_replay_without_device_session() {
        let setup = setup();
        let key = dpop_key();
        let code = crate::request::generate_code();
        let jkt = key.to_public().thumbprint();
        let token_data = TokenData {
            created_at: NOW,
            updated_at: NOW,
            expires_at: NOW + TOKEN_MAX_AGE,
            client_id: CLIENT_ID.to_string(),
            client_auth: ClientAuth::None,
            device_id: None,
            did: "did:plc:alice".to_string(),
            parameters: AuthorizationRequestParameters {
                client_id: CLIENT_ID.to_string(),
                response_type: "code".to_string(),
                redirect_uri: "https://app.example.com/callback".to_string(),
                scope: "atproto".to_string(),
                state: None,
                code_challenge: PKCE_CHALLENGE.to_string(),
                code_challenge_method: CODE_CHALLENGE_METHOD_S256.to_string(),
                login_hint: None,
                prompt: None,
                dpop_jkt: Some(jkt),
            },
            code: Some(code.clone()),
        };
        setup
            .store
            .create_token("tok-11111111111111111111111111111111", &token_data, None)
            .await
            .unwrap();
        let err = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("invalid code"));
        assert!(setup
            .store
            .read_token("tok-11111111111111111111111111111111")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn jwks_uri_fetch_failure_rejects_client() {
        let mut metadata = confidential_metadata(CLIENT_ID);
        metadata.jwks = None;
        metadata.jwks_uri = Some("https://app.example.com/jwks.json".to_string());
        let setup = setup_with(vec![metadata], None);
        let key = dpop_key();
        let htu = format!("{ISSUER}/oauth/par");
        let par_proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [par_proof.as_str()];
        let err = setup
            .provider
            .pushed_authorization_request(
                &credentials(CLIENT_ID),
                &par_request(CLIENT_ID),
                &DpopRequest {
                    method: "POST",
                    uri: &htu,
                    dpop_headers: &headers,
                    access_token: None,
                },
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("no jwks"));
    }

    #[tokio::test]
    async fn token_grant_requires_client_grant_registration() {
        // client without refresh_token grant gets no refresh token at all
        let mut metadata = public_metadata(CLIENT_ID);
        metadata.grant_types = vec![GRANT_AUTHORIZATION_CODE.to_string()];
        let setup = setup_with(vec![metadata], None);
        let key = dpop_key();
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        assert!(tokens.refresh_token.is_none());
    }

    #[test]
    fn request_structs_derive_helpers() {
        let credentials = ClientCredentials::default();
        assert!(credentials.client_id.is_empty());
        assert_eq!(credentials.clone(), credentials);
        let request = TokenRequest::default();
        assert!(request.grant_type.is_empty());
        assert_eq!(request.clone(), request);
        let page = AuthorizePageData {
            request_uri: "urn:x".to_string(),
            client_id: "client".to_string(),
            client_name: None,
            client_uri: None,
            logo_uri: None,
            client_trusted: false,
            scopes: vec![],
            login_hint: None,
            prompt: None,
            sessions: vec![],
        };
        assert_eq!(page.clone(), page);
        let access = VerifiedAccess {
            did: "did:plc:alice".to_string(),
            scopes: vec!["atproto".to_string()],
            token_id: "tok-x".to_string(),
        };
        assert_eq!(access.clone(), access);
        assert!(!format!("{page:?}{access:?}{credentials:?}{request:?}").is_empty());
    }

    #[tokio::test]
    async fn metadata_documents() {
        let setup = setup();
        let metadata = setup.provider.authorization_server_metadata();
        assert_eq!(metadata["issuer"], ISSUER);
        assert_eq!(
            metadata["pushed_authorization_request_endpoint"],
            format!("{ISSUER}/oauth/par")
        );
        assert_eq!(metadata["require_pushed_authorization_requests"], true);
        assert_eq!(
            metadata["authorization_response_iss_parameter_supported"],
            true
        );
        assert_eq!(metadata["client_id_metadata_document_supported"], true);
        assert_eq!(
            metadata["code_challenge_methods_supported"],
            json!(["S256"])
        );
        assert_eq!(
            metadata["grant_types_supported"],
            json!(["authorization_code", "refresh_token"])
        );
        assert_eq!(metadata["scopes_supported"][0], json!("atproto"));
        assert_eq!(
            metadata["token_endpoint_auth_methods_supported"],
            json!(["none", "private_key_jwt"])
        );
        assert_eq!(
            metadata["dpop_signing_alg_values_supported"],
            json!(["ES256", "ES256K"])
        );

        let resource = setup.provider.protected_resource_metadata();
        assert_eq!(resource["resource"], ISSUER);
        assert_eq!(resource["authorization_servers"], json!([ISSUER]));

        let jwks = setup.provider.jwks();
        assert_eq!(jwks.keys.len(), 1);
        assert!(!jwks.keys[0].is_private());

        assert_eq!(setup.provider.issuer(), ISSUER);
        assert!(setup.provider.next_dpop_nonce(NOW).is_none());
        assert_eq!(
            setup
                .provider
                .store()
                .get_account("did:plc:alice")
                .await
                .unwrap()
                .unwrap()
                .did,
            "did:plc:alice"
        );

        let par_response = ParResponse {
            request_uri: "urn:ietf:params:oauth:request_uri:req-x".to_string(),
            expires_in: 300,
        };
        let value = serde_json::to_value(&par_response).unwrap();
        assert_eq!(value["expires_in"], 300);
        let parsed: ParResponse = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, par_response);
    }
}
