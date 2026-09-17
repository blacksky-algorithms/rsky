use crate::client::{Client, ClientManager, ClientMetadataFetcher, ParRequest};
use crate::device::{generate_session_id, AUTHENTICATION_MAX_AGE, EPHEMERAL_SESSION_MAX_AGE};
use crate::dpop::{DpopManager, DpopProof, DpopRequest};
use crate::error::OAuthError;
use crate::jwk::{JwkSet, SigningKey};
use crate::jwt;
use crate::jwt::{JwtClaims, JwtHeader};
use crate::request::{
    generate_code, generate_request_id, is_code, request_id_from_uri, request_uri_from_id,
    RequestData, AUTHORIZATION_INACTIVITY_TIMEOUT, PAR_EXPIRES_IN,
};
use crate::store::{AccountInfo, DeviceAccount, OAuthStore};
use crate::token::{
    generate_refresh_token, generate_token_id, is_refresh_token, is_token_id,
    verify_code_challenge, TokenData, TokenInfo, TOKEN_MAX_AGE,
};
use crate::types::*;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use url::Url;

/// A permission set named by the scope could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeExpandError(pub String);

impl fmt::Display for ScopeExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ScopeExpandError {}

/// Resolves permission-set `include:` scopes into the effective grants they
/// confer, so the issued access token's `scope` claim reflects what the session
/// can actually do rather than the unresolved reference. Async because
/// resolution reaches the network; a no-op returns the scope unchanged.
#[async_trait::async_trait]
pub trait ScopeExpander: Send + Sync {
    async fn expand(&self, granted_scope: &str) -> Result<String, ScopeExpandError>;
}

pub const ACCESS_TOKEN_TYP: &str = "at+jwt";
/// Audience prefix of ephemeral sign-in proofs, followed by the issuer.
pub const EPHEMERAL_TOKEN_AUDIENCE_PREFIX: &str = "oauth-provider-api@";
const PERMISSION_SETS_UNAVAILABLE: &str = "Unable to retrieve permission sets";

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

/// An account signed in on the device, with what the authorization flow
/// still needs from it.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub account: AccountInfo,
    pub login_required: bool,
    pub consent_required: bool,
}

/// Everything the host application needs to render the authorization UI.
/// The provider never produces HTML.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuthorizePageData {
    pub request_uri: String,
    pub client_id: String,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub client_trusted: bool,
    pub scopes: Vec<String>,
    pub login_hint: Option<String>,
    pub prompt: Option<String>,
    /// Accounts already signed in on this device.
    pub sessions: Vec<SessionInfo>,
    /// The session the page should offer first, when the prompt allows one.
    pub selected_did: Option<String>,
}

/// What `GET /oauth/authorize` should answer with.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthorizeOutcome {
    /// Send the browser to the client: a code, or an error the client asked
    /// to receive instead of a page.
    Redirect(String),
    Page(Box<AuthorizePageData>),
}

impl AuthorizeOutcome {
    pub fn into_page(self) -> Option<Box<AuthorizePageData>> {
        match self {
            Self::Page(page) => Some(page),
            Self::Redirect(_) => None,
        }
    }

    pub fn into_redirect(self) -> Option<String> {
        match self {
            Self::Redirect(url) => Some(url),
            Self::Page(_) => None,
        }
    }
}

/// How the browser proves it may act for the account it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountProof<'a> {
    /// The device session secret presented with the request.
    Device { session_id: &'a str },
    /// A signed proof from a sign-in that was not remembered.
    Ephemeral(&'a str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignInResult {
    pub account: AccountInfo,
    /// Present when the sign-in was not remembered on the device.
    pub ephemeral_token: Option<String>,
    /// Present when the sign-in was remembered: the device session id the
    /// cookie must carry from now on.
    pub new_session_id: Option<String>,
}

/// A live OAuth session of an account, as shown on its account pages.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveOAuthSession {
    pub token_id: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// Whether the access token is still within its lifetime.
    pub active: bool,
    pub client_id: String,
    pub client_metadata: Option<OAuthClientMetadata>,
    pub scope: Option<String>,
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
    /// The key access tokens are signed with.
    pub signing_key: SigningKey,
    pub fetcher: Arc<dyn ClientMetadataFetcher>,
    pub store: Arc<dyn OAuthStore>,
    pub dpop: DpopManager,
    /// client_ids treated as trusted (first-party) for UI purposes.
    pub trusted_clients: Vec<String>,
    /// Expands `include:` permission sets into the token's effective scope.
    /// `None` leaves the granted scope unexpanded.
    pub scope_expander: Option<Arc<dyn ScopeExpander>>,
    /// Device authentications older than this instant (unix seconds) must
    /// sign in again, whatever their age.
    pub sessions_since: Option<u64>,
}

pub struct OAuthProvider {
    issuer: String,
    audience: String,
    signing_key: SigningKey,
    clients: ClientManager,
    store: Arc<dyn OAuthStore>,
    dpop: DpopManager,
    trusted_clients: Vec<String>,
    scope_expander: Option<Arc<dyn ScopeExpander>>,
    sessions_since: Option<u64>,
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
            scope_expander: config.scope_expander,
            sessions_since: config.sessions_since,
        }
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn store(&self) -> &Arc<dyn OAuthStore> {
        &self.store
    }

    /// Whether `client_id` is configured as trusted (first-party) via
    /// `trusted_clients` -- the same check used to compute
    /// [`AuthorizePageData::client_trusted`] for the consent-page UI.
    pub fn is_trusted_client(&self, client_id: &str) -> bool {
        self.trusted_clients.iter().any(|id| id == client_id)
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

    /// The grants `scope` confers once every permission set in it is
    /// resolved. Fails closed: an unresolvable set must not become authority.
    async fn compile_scope(&self, scope: &str) -> Result<String, OAuthError> {
        match &self.scope_expander {
            Some(expander) => expander.expand(scope).await.map_err(|error| {
                tracing::warn!(%error, "permission set resolution failed");
                OAuthError::InvalidScope(PERMISSION_SETS_UNAVAILABLE.to_string())
            }),
            None => Ok(scope.to_string()),
        }
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
        let proof = self.dpop.check_proof(dpop, Some(&client.id), now).await?;
        let mut parameters =
            client.validate_request(request, self.is_trusted_client(&client.id))?;
        self.compile_scope(&parameters.scope).await?;
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

    /// Whether the account must authenticate again before this device
    /// session may act for it.
    pub fn check_login_required(&self, device_account: &DeviceAccount, now: u64) -> bool {
        now.saturating_sub(device_account.updated_at) > AUTHENTICATION_MAX_AGE
            || self
                .sessions_since
                .is_some_and(|since| device_account.updated_at < since)
    }

    /// Whether the consent screen must be shown: always without a prior
    /// grant or when the client asks for it, otherwise only when the request
    /// reaches beyond what the account already authorized for this client.
    pub fn check_consent_required(
        parameters: &AuthorizationRequestParameters,
        authorized_scope: Option<&str>,
    ) -> bool {
        let Some(authorized) = authorized_scope else {
            return true;
        };
        if parameters.prompt.as_deref() == Some("consent") {
            return true;
        }
        let authorized: HashSet<&str> = authorized.split_ascii_whitespace().collect();
        !parameters
            .scope
            .split_ascii_whitespace()
            .all(|scope| authorized.contains(scope))
    }

    /// GET /oauth/authorize: decides between a page and a redirect the way
    /// the reference PDS does.
    pub async fn authorize(
        &self,
        client_id: &str,
        request_uri: &str,
        device_id: &str,
        now: u64,
    ) -> Result<AuthorizeOutcome, OAuthError> {
        let (request_id, mut data) = self
            .active_request(client_id, request_uri, device_id, now)
            .await?;
        let client = self.clients.get_client(client_id).await?;
        let mut sessions = Vec::new();
        for linked in self.store.list_device_accounts(device_id).await? {
            let login_required = data.parameters.prompt.as_deref() == Some("login")
                || self.check_login_required(&linked, now);
            let authorized = self
                .store
                .get_authorized_client_scope(&linked.account.did, client_id)
                .await?;
            sessions.push(SessionInfo {
                consent_required: Self::check_consent_required(
                    &data.parameters,
                    authorized.as_deref(),
                ),
                account: linked.account,
                login_required,
            });
        }
        let hint = data.parameters.login_hint.as_deref();
        let sso: Vec<&SessionInfo> = sessions
            .iter()
            .filter(|session| {
                !session.account.deactivated && session_matches_hint(&session.account, hint)
            })
            .collect();
        let prompt = data.parameters.prompt.as_deref();
        if prompt == Some("select_account") && sessions.is_empty() {
            return self
                .authorization_error_redirect(
                    &request_id,
                    &data.parameters,
                    "account_selection_required",
                    "Account selection required",
                )
                .await
                .map(AuthorizeOutcome::Redirect);
        }
        if prompt == Some("none") {
            let (error, description) = match sso.as_slice() {
                [] => ("login_required", "Login required"),
                [session] if session.login_required => ("login_required", "Login required"),
                [session] if session.consent_required => ("consent_required", "Consent required"),
                [session] => {
                    let did = session.account.did.clone();
                    return self
                        .silent_authorize(&request_id, &mut data, &did, now)
                        .await;
                }
                _ => ("account_selection_required", "Account selection required"),
            };
            return self
                .authorization_error_redirect(&request_id, &data.parameters, error, description)
                .await
                .map(AuthorizeOutcome::Redirect);
        }
        if prompt.is_none() && hint.is_some() {
            if let [session] = sso.as_slice() {
                if !session.login_required && !session.consent_required {
                    let did = session.account.did.clone();
                    return self
                        .silent_authorize(&request_id, &mut data, &did, now)
                        .await;
                }
            }
        }
        let selected_did = if matches!(prompt, None | Some("login") | Some("consent")) {
            sessions
                .iter()
                .find(|session| session_matches_hint(&session.account, hint))
                .map(|session| session.account.did.clone())
        } else {
            None
        };
        Ok(AuthorizeOutcome::Page(Box::new(AuthorizePageData {
            request_uri: request_uri.to_string(),
            client_id: client.id.clone(),
            client_name: client.metadata.client_name.clone(),
            client_uri: client.metadata.client_uri.clone(),
            logo_uri: client.metadata.logo_uri.clone(),
            tos_uri: client.metadata.tos_uri.clone(),
            policy_uri: client.metadata.policy_uri.clone(),
            client_trusted: self.is_trusted_client(&client.id),
            scopes: data
                .parameters
                .scope
                .split_ascii_whitespace()
                .map(String::from)
                .collect(),
            login_hint: data.parameters.login_hint.clone(),
            prompt: data.parameters.prompt.clone(),
            sessions,
            selected_did,
        })))
    }

    /// Issues the code without a consent screen, or sends the client the
    /// `invalid_scope` error when the grants cannot be established.
    async fn silent_authorize(
        &self,
        request_id: &str,
        data: &mut RequestData,
        did: &str,
        now: u64,
    ) -> Result<AuthorizeOutcome, OAuthError> {
        if let Err(error) = self.compile_scope(&data.parameters.scope).await {
            return self
                .authorization_error_redirect(
                    request_id,
                    &data.parameters,
                    error.error_code(),
                    error.error_description(),
                )
                .await
                .map(AuthorizeOutcome::Redirect);
        }
        let code = self
            .set_authorized(request_id, data, did, None, now)
            .await?;
        self.build_redirect(&data.parameters, &[("code", &code)])
            .map(AuthorizeOutcome::Redirect)
    }

    /// Discards the request and returns the error redirect for the client.
    async fn authorization_error_redirect(
        &self,
        request_id: &str,
        parameters: &AuthorizationRequestParameters,
        error: &str,
        description: &str,
    ) -> Result<String, OAuthError> {
        self.store.delete_request(request_id).await?;
        self.build_redirect(
            parameters,
            &[("error", error), ("error_description", description)],
        )
    }

    /// Marks the request authorized for `did` and mints its code. A
    /// `granted_scope` narrows the request to what the user accepted; it
    /// must stay within the requested scope and keep `atproto`.
    async fn set_authorized(
        &self,
        request_id: &str,
        data: &mut RequestData,
        did: &str,
        granted_scope: Option<&str>,
        now: u64,
    ) -> Result<String, OAuthError> {
        if let Some(granted) = granted_scope {
            let requested: HashSet<&str> = data.parameters.scope.split_ascii_whitespace().collect();
            let granted: Vec<&str> = granted.split_ascii_whitespace().collect();
            if !granted.contains(&SCOPE_ATPROTO)
                || granted.iter().any(|scope| !requested.contains(scope))
            {
                return Err(OAuthError::InvalidRequest(format!(
                    "granted scope must be within the requested scope and include \"{SCOPE_ATPROTO}\""
                )));
            }
            data.parameters.scope = granted.join(" ");
        }
        let code = generate_code();
        data.did = Some(did.to_string());
        data.code = Some(code.clone());
        data.expires_at = now + AUTHORIZATION_INACTIVITY_TIMEOUT;
        self.store.update_request(request_id, data).await?;
        Ok(code)
    }

    /// POST sign-in during the authorization flow. A remembered sign-in
    /// rotates the device session and links the account to the device in one
    /// step; otherwise the account is unlinked and a short-lived proof is
    /// returned for the acceptance step.
    #[allow(clippy::too_many_arguments)]
    pub async fn sign_in(
        &self,
        client_id: &str,
        request_uri: &str,
        device_id: &str,
        identifier: &str,
        password: &str,
        remember: bool,
        presented_session_id: &str,
        now: u64,
    ) -> Result<SignInResult, OAuthError> {
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
        if remember {
            let new_session_id = generate_session_id();
            self.store
                .authenticate_device_account(
                    device_id,
                    presented_session_id,
                    &new_session_id,
                    &account.did,
                    now,
                )
                .await?;
            return Ok(SignInResult {
                account,
                ephemeral_token: None,
                new_session_id: Some(new_session_id),
            });
        }
        self.store
            .remove_device_account(device_id, &account.did)
            .await?;
        let ephemeral_token =
            self.create_ephemeral_token(&account.did, device_id, request_uri, now)?;
        Ok(SignInResult {
            account,
            ephemeral_token: Some(ephemeral_token),
            new_session_id: None,
        })
    }

    fn ephemeral_audience(&self) -> String {
        format!("{EPHEMERAL_TOKEN_AUDIENCE_PREFIX}{}", self.issuer)
    }

    /// A signed proof that `did` authenticated on `device_id` for this
    /// request without being remembered, valid for
    /// [`EPHEMERAL_SESSION_MAX_AGE`].
    pub fn create_ephemeral_token(
        &self,
        did: &str,
        device_id: &str,
        request_uri: &str,
        now: u64,
    ) -> Result<String, OAuthError> {
        let mut header = JwtHeader::new(self.signing_key.alg()?);
        header.typ = Some(ACCESS_TOKEN_TYP.to_string());
        let mut claims = JwtClaims {
            iss: Some(self.issuer.clone()),
            aud: Some(Value::String(self.ephemeral_audience())),
            sub: Some(did.to_string()),
            iat: Some(now),
            exp: Some(now + EPHEMERAL_SESSION_MAX_AGE),
            ..Default::default()
        };
        claims
            .extra
            .insert("device_id".to_string(), json!(device_id));
        claims
            .extra
            .insert("request_uri".to_string(), json!(request_uri));
        jwt::sign_with(&header, &claims, &self.signing_key)
    }

    /// Checks an ephemeral proof against the account, device and request it
    /// must have been issued for.
    pub fn verify_ephemeral_token(
        &self,
        token: &str,
        did: &str,
        device_id: &str,
        request_uri: &str,
        now: u64,
    ) -> Result<(), OAuthError> {
        let decoded = jwt::verify_with(token, &self.signing_key)?;
        decoded.header.validate_typ(ACCESS_TOKEN_TYP)?;
        let claims = &decoded.claims;
        claims.validate_iss(&self.issuer)?;
        claims.validate_aud(&self.ephemeral_audience())?;
        claims.validate_time(now, 0)?;
        let claim = |name: &str| claims.extra.get(name).and_then(Value::as_str);
        if claims.sub.as_deref() != Some(did)
            || claim("device_id") != Some(device_id)
            || claim("request_uri") != Some(request_uri)
        {
            return Err(OAuthError::InvalidRequest(
                "session token does not match this request".to_string(),
            ));
        }
        Ok(())
    }

    /// Consent accepted: issues the authorization code and returns the
    /// redirect URL for the client callback. The client's authorized scope
    /// is recorded only when consent was actually asked for.
    #[allow(clippy::too_many_arguments)]
    pub async fn accept(
        &self,
        client_id: &str,
        request_uri: &str,
        device_id: &str,
        did: &str,
        proof: AccountProof<'_>,
        granted_scope: Option<&str>,
        now: u64,
    ) -> Result<String, OAuthError> {
        let (request_id, mut data) = self
            .active_request(client_id, request_uri, device_id, now)
            .await?;
        match proof {
            AccountProof::Device { session_id } => {
                let Some(linked) = self
                    .store
                    .get_device_account_for_session(device_id, session_id, did)
                    .await?
                else {
                    return Err(OAuthError::InvalidRequest(
                        "account is not signed in on this device".to_string(),
                    ));
                };
                if self.check_login_required(&linked, now) {
                    return Err(OAuthError::InvalidRequest("login required".to_string()));
                }
            }
            AccountProof::Ephemeral(token) => {
                self.verify_ephemeral_token(token, did, device_id, request_uri, now)?
            }
        }
        let authorized = self
            .store
            .get_authorized_client_scope(did, client_id)
            .await?;
        let consent_required =
            Self::check_consent_required(&data.parameters, authorized.as_deref());
        if let Err(error) = self
            .compile_scope(granted_scope.unwrap_or(&data.parameters.scope))
            .await
        {
            return self
                .authorization_error_redirect(
                    &request_id,
                    &data.parameters,
                    error.error_code(),
                    error.error_description(),
                )
                .await;
        }
        let code = self
            .set_authorized(&request_id, &mut data, did, granted_scope, now)
            .await?;
        if consent_required {
            let mut union: Vec<&str> = authorized
                .as_deref()
                .unwrap_or_default()
                .split_ascii_whitespace()
                .collect();
            for scope in data.parameters.scope.split_ascii_whitespace() {
                if !union.contains(&scope) {
                    union.push(scope);
                }
            }
            self.store
                .set_authorized_client(did, client_id, &union.join(" "))
                .await?;
        }
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
        self.authorization_error_redirect(
            &request_id,
            &data.parameters,
            "access_denied",
            "Access denied",
        )
        .await
    }

    fn build_redirect(
        &self,
        parameters: &AuthorizationRequestParameters,
        pairs: &[(&str, &str)],
    ) -> Result<String, OAuthError> {
        let mut url = Url::parse(&parameters.redirect_uri)
            .map_err(|_| OAuthError::ServerError("stored redirect_uri is invalid".to_string()))?;
        let fragment_mode = parameters.response_mode.as_deref() == Some("fragment");
        if fragment_mode {
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (key, value) in pairs {
                serializer.append_pair(key, value);
            }
            if let Some(state) = &parameters.state {
                serializer.append_pair("state", state);
            }
            serializer.append_pair("iss", &self.issuer);
            url.set_fragment(Some(&serializer.finish()));
        } else {
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

    /// The OAuth sessions of an account that can still be used, newest
    /// first. Client metadata is fetched best-effort for display.
    pub async fn list_account_sessions(
        &self,
        did: &str,
        now: u64,
    ) -> Result<Vec<ActiveOAuthSession>, OAuthError> {
        let mut sessions = Vec::new();
        for token in self.store.list_account_tokens(did).await? {
            let client_id = token.data.client_id.clone();
            let client_metadata = match self.clients.get_client(&client_id).await {
                Ok(client) => Some(client.metadata),
                Err(error) => {
                    tracing::debug!(%client_id, %error, "client metadata unavailable");
                    None
                }
            };
            // A client whose metadata cannot be read is assumed able to
            // refresh, so the session stays visible and revocable.
            let can_refresh = client_metadata.as_ref().is_none_or(|metadata| {
                metadata
                    .grant_types
                    .iter()
                    .any(|grant| grant == GRANT_REFRESH_TOKEN)
            });
            let usable = if can_refresh {
                token
                    .data
                    .validate_refresh_lifetimes(now, self.is_trusted_client(&client_id))
                    .is_ok()
            } else {
                token.data.expires_at > now
            };
            if !usable {
                continue;
            }
            sessions.push(ActiveOAuthSession {
                token_id: token.token_id,
                created_at: token.data.created_at,
                updated_at: token.data.updated_at,
                active: token.data.expires_at > now,
                client_id,
                client_metadata,
                scope: token.data.scope,
            });
        }
        Ok(sessions)
    }

    /// Revokes one of the account's own sessions.
    pub async fn revoke_account_token(&self, did: &str, token_id: &str) -> Result<(), OAuthError> {
        match self.store.read_token(token_id).await? {
            Some(token) if token.data.did == did => self.store.delete_token(token_id).await,
            _ => Err(OAuthError::InvalidRequest("Invalid token".to_string())),
        }
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
        let Some(proof) = self.dpop.check_proof(dpop, Some(&client.id), now).await? else {
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
        // The client metadata was validated against the literal requested
        // scope (with `include:`); the granted scope stored with the session
        // is the compiled form, so a resource server sees the effective
        // grants and refreshes stay consistent. The requested parameters are
        // kept as they were, the way the reference PDS stores them.
        let granted_scope = self.compile_scope(&parameters.scope).await?;
        let token_data = TokenData {
            created_at: now,
            updated_at: now,
            expires_at: now + TOKEN_MAX_AGE,
            client_id: client.id.clone(),
            client_auth,
            device_id: data.device_id.clone(),
            did: account.did.clone(),
            parameters,
            code: Some(code.to_string()),
            scope: Some(granted_scope),
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
                "Refresh token replayed".to_string(),
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
        token
            .data
            .validate_refresh_lifetimes(now, self.trusted_clients.contains(&client.id))?;
        // The grants are recompiled from the request the user accepted so a
        // changed permission set takes effect at refresh; when the sets cannot
        // be resolved the session keeps what it had rather than dying.
        let scope = match self.compile_scope(&token.data.parameters.scope).await {
            Ok(scope) => Some(scope),
            Err(_) => {
                tracing::info!(token_id = %token.token_id, "keeping the session scope unchanged");
                None
            }
        };
        let new_token_id = generate_token_id();
        let new_refresh_token = generate_refresh_token();
        self.store
            .rotate_token(
                &token.token_id,
                &new_token_id,
                &new_refresh_token,
                now,
                now + TOKEN_MAX_AGE,
                scope.as_deref(),
            )
            .await?;
        let mut data = token.data.clone();
        data.updated_at = now;
        data.expires_at = now + TOKEN_MAX_AGE;
        if scope.is_some() {
            data.scope = scope;
        }
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
        // The same claim set as the reference PDS's stateful tokens: the
        // scope lives with the stored session, not in the token.
        let mut header = JwtHeader::new(self.signing_key.alg()?);
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
        claims.extra.insert("cnf".to_string(), json!({"jkt": jkt}));
        claims
            .extra
            .insert("client_id".to_string(), json!(data.client_id));
        let access_token = jwt::sign_with(&header, &claims, &self.signing_key)?;
        Ok(TokenResponse {
            access_token,
            token_type: "DPoP".to_string(),
            expires_in: TOKEN_MAX_AGE,
            refresh_token,
            scope: data.granted_scope().to_string(),
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
    /// Verifies a DPoP-bound access token the way the reference PDS does in
    /// its stateful mode: the signature and standard claims must hold, and
    /// then the stored session is authoritative for the key binding, the
    /// expiry, and the granted scope, so revocation and rotation take effect
    /// immediately.
    pub async fn verify_access_token(
        &self,
        access_token: &str,
        dpop: &DpopRequest<'_>,
        now: u64,
    ) -> Result<VerifiedAccess, OAuthError> {
        let decoded = jwt::verify_with(access_token, &self.signing_key)?;
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
        let Some(stored) = self.store.read_token(&token_id).await? else {
            return Err(OAuthError::InvalidToken("Invalid token".to_string()));
        };
        if stored.data.did != did || stored.data.parameters.dpop_jkt.as_deref() != Some(jkt) {
            self.store.delete_token(&token_id).await?;
            return Err(OAuthError::InvalidToken("Invalid token".to_string()));
        }
        if stored.data.expires_at <= now {
            self.store.delete_token(&token_id).await?;
            return Err(OAuthError::InvalidToken("Token expired".to_string()));
        }
        let scopes: Vec<String> = stored
            .data
            .granted_scope()
            .split_ascii_whitespace()
            .map(String::from)
            .collect();
        if !scopes.iter().any(|scope| scope == SCOPE_ATPROTO) {
            return Err(OAuthError::InvalidToken(format!(
                "access token is missing the \"{SCOPE_ATPROTO}\" scope"
            )));
        }
        let Some(proof) = self.dpop.check_proof(dpop, None, now).await? else {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP proof is required".to_string(),
            ));
        };
        if proof.jkt != jkt {
            return Err(OAuthError::InvalidToken(
                "DPoP key does not match the token binding".to_string(),
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
        self.signing_key.public_jwks()
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
            "response_modes_supported": ["query", "fragment"],
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

/// Without a hint no session matches, as in the reference: the silent paths
/// and the preselection need the client to name the account.
fn session_matches_hint(account: &AccountInfo, hint: Option<&str>) -> bool {
    hint.is_some_and(|hint| account_matches_hint(account, hint))
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
    use crate::jwk::{EcCurve, Jwk};
    use crate::store::{DeviceData, MemoryOAuthStore};
    use crate::types::{
        AUTH_METHOD_PRIVATE_KEY_JWT, CLIENT_ASSERTION_TYPE_JWT_BEARER, CODE_CHALLENGE_METHOD_S256,
    };
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    const NOW: u64 = 1_700_000_000;
    const ISSUER: &str = "https://pds.example.com";
    const AUDIENCE: &str = "did:web:pds.example.com";
    const CLIENT_ID: &str = "https://app.example.com/oauth/client-metadata.json";
    const OTHER_CLIENT_ID: &str = "https://other.example.com/oauth/client-metadata.json";
    const DEVICE: &str = "dev-00000000000000000000000000000000";
    const SESSION: &str = "ses-00000000000000000000000000000000";
    const INCLUDE_CLIENT_ID: &str = "https://sets.example.com/oauth/client-metadata.json";
    const INCLUDE_SCOPE: &str = "atproto transition:generic include:app.example.set";
    const PKCE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const PKCE_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    static JTI: AtomicU64 = AtomicU64::new(0);

    fn signing_jwk() -> Jwk {
        Jwk::from_private_key_bytes(EcCurve::K256, &[0x42u8; 32]).unwrap()
    }

    fn signing_key() -> SigningKey {
        SigningKey::Ec(signing_jwk())
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

    fn include_metadata() -> OAuthClientMetadata {
        let mut metadata = public_metadata(INCLUDE_CLIENT_ID);
        metadata.scope = Some(INCLUDE_SCOPE.to_string());
        metadata
    }

    const COMPILED_SET: &str = "repo:app.example.record";
    const RECOMPILED_SET: &str = "repo:app.example.other";

    /// Replaces every `include:` with a fixed compiled grant (a different one
    /// once the set "changed"), or fails when told to.
    #[derive(Default)]
    struct StubExpander {
        fail: AtomicBool,
        changed: AtomicBool,
    }

    #[async_trait::async_trait]
    impl ScopeExpander for StubExpander {
        async fn expand(&self, granted_scope: &str) -> Result<String, ScopeExpandError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(ScopeExpandError("resolver unreachable".to_string()));
            }
            let compiled = if self.changed.load(Ordering::SeqCst) {
                RECOMPILED_SET
            } else {
                COMPILED_SET
            };
            Ok(granted_scope
                .split_ascii_whitespace()
                .map(|scope| {
                    if scope.starts_with("include:") {
                        compiled
                    } else {
                        scope
                    }
                })
                .collect::<Vec<_>>()
                .join(" "))
        }
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
        expander: Arc<StubExpander>,
    }

    #[derive(Default)]
    struct SetupOptions {
        nonce: Option<DpopNonce>,
        trusted: Vec<String>,
        sessions_since: Option<u64>,
        expander: bool,
    }

    fn account(did: &str, handle: &str, email: Option<&str>) -> AccountInfo {
        AccountInfo {
            did: did.to_string(),
            handle: Some(handle.to_string()),
            email: email.map(String::from),
            email_verified: false,
            deactivated: false,
        }
    }

    fn build_setup(clients: Vec<OAuthClientMetadata>, options: SetupOptions) -> Setup {
        let store = Arc::new(MemoryOAuthStore::new());
        store.add_account(
            account(
                "did:plc:alice",
                "alice.example.com",
                Some("alice@example.com"),
            ),
            "correct-password",
        );
        store.add_account(
            account("did:plc:bob", "bob.example.com", None),
            "bobs-password",
        );
        let mut carol = account("did:plc:carol", "carol.example.com", None);
        carol.deactivated = true;
        store.add_account(carol, "carols-password");
        let expander = Arc::new(StubExpander::default());
        let provider = OAuthProvider::new(OAuthProviderConfig {
            issuer: ISSUER.to_string(),
            audience: AUDIENCE.to_string(),
            signing_key: signing_key(),
            fetcher: Arc::new(StubFetcher { clients }),
            store: store.clone(),
            dpop: DpopManager::new(options.nonce, Box::new(InMemoryReplayStore::default())),
            trusted_clients: options.trusted,
            scope_expander: options
                .expander
                .then(|| expander.clone() as Arc<dyn ScopeExpander>),
            sessions_since: options.sessions_since,
        });
        Setup {
            provider,
            store,
            expander,
        }
    }

    fn setup_with(clients: Vec<OAuthClientMetadata>, nonce: Option<DpopNonce>) -> Setup {
        build_setup(
            clients,
            SetupOptions {
                nonce,
                ..Default::default()
            },
        )
    }

    fn setup() -> Setup {
        setup_with(
            vec![public_metadata(CLIENT_ID), public_metadata(OTHER_CLIENT_ID)],
            None,
        )
    }

    fn setup_trusted() -> Setup {
        build_setup(
            vec![
                public_metadata(CLIENT_ID),
                public_metadata(OTHER_CLIENT_ID),
                include_metadata(),
            ],
            SetupOptions {
                trusted: vec![CLIENT_ID.to_string(), INCLUDE_CLIENT_ID.to_string()],
                expander: true,
                ..Default::default()
            },
        )
    }

    /// Creates the test device when absent; returns its current session id.
    async fn device_session(setup: &Setup) -> String {
        match setup.store.read_device(DEVICE).await.unwrap() {
            Some(device) => device.session_id,
            None => {
                setup
                    .store
                    .create_device(
                        DEVICE,
                        &DeviceData {
                            session_id: SESSION.to_string(),
                            user_agent: None,
                            ip_address: "127.0.0.1".to_string(),
                            last_seen_at: NOW,
                        },
                    )
                    .await
                    .unwrap();
                SESSION.to_string()
            }
        }
    }

    /// A remembered sign-in presenting the device's current session id.
    async fn sign_in(
        setup: &Setup,
        client_id: &str,
        request_uri: &str,
        identifier: &str,
        password: &str,
        now: u64,
    ) -> Result<SignInResult, OAuthError> {
        let session_id = device_session(setup).await;
        setup
            .provider
            .sign_in(
                client_id,
                request_uri,
                DEVICE,
                identifier,
                password,
                true,
                &session_id,
                now,
            )
            .await
    }

    /// Accepts with the device's current session id as proof.
    async fn accept(
        setup: &Setup,
        client_id: &str,
        request_uri: &str,
        did: &str,
        granted_scope: Option<&str>,
        now: u64,
    ) -> Result<String, OAuthError> {
        let session_id = device_session(setup).await;
        setup
            .provider
            .accept(
                client_id,
                request_uri,
                DEVICE,
                did,
                AccountProof::Device {
                    session_id: &session_id,
                },
                granted_scope,
                now,
            )
            .await
    }

    fn expect_page(outcome: AuthorizeOutcome) -> AuthorizePageData {
        *outcome.into_page().expect("a page")
    }

    fn expect_redirect(outcome: AuthorizeOutcome) -> Url {
        Url::parse(&outcome.into_redirect().expect("a redirect")).unwrap()
    }

    /// Installs a subscriber so the log statements on the paths under test
    /// run rather than being skipped as disabled.
    fn init_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .try_init();
    }

    fn query_value(url: &Url, key: &str) -> Option<String> {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, value)| value.into_owned())
    }

    async fn run_par_with(
        setup: &Setup,
        key: &Jwk,
        now: u64,
        mutate: impl FnOnce(&mut ParRequest),
    ) -> ParResponse {
        let htu = format!("{ISSUER}/oauth/par");
        let proof = proof(key, "POST", &htu, now, None, None);
        let headers = [proof.as_str()];
        let mut request = par_request(CLIENT_ID);
        mutate(&mut request);
        let client_id = request.client_id.clone();
        setup
            .provider
            .pushed_authorization_request(
                &credentials(&client_id),
                &request,
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
            response_mode: None,
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
        let page = expect_page(
            setup
                .provider
                .authorize(client_id, &par.request_uri, DEVICE, now)
                .await
                .unwrap(),
        );
        assert_eq!(page.client_id, client_id);
        sign_in(
            setup,
            client_id,
            &par.request_uri,
            "alice.example.com",
            "correct-password",
            now,
        )
        .await
        .unwrap();
        let redirect = accept(
            setup,
            client_id,
            &par.request_uri,
            "did:plc:alice",
            None,
            now,
        )
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
        assert_eq!(err.error_description(), "Invalid token");
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
        assert_eq!(err.error_description(), "Invalid token");
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
        expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            "correct-password",
            NOW,
        )
        .await
        .unwrap();
        let redirect = accept(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
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
        let page = expect_page(
            setup
                .provider
                .authorize(client_id, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(page.scopes, vec!["atproto"]);
        assert!(!page.client_trusted);
        assert!(page.tos_uri.is_none());
        assert!(page.selected_did.is_none());
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
        sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "alice.example.com",
            "correct-password",
            NOW,
        )
        .await
        .unwrap();
        accept(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
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
        let err = sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
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
        let err = sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "bob.example.com",
            "bobs-password",
            NOW,
        )
        .await
        .unwrap_err();
        assert!(err.error_description().contains("login_hint"));
        sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "alice.example.com",
            "correct-password",
            NOW,
        )
        .await
        .unwrap();
        // the page lists the session and preselects the hinted account
        let page = expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(page.sessions.len(), 1);
        assert_eq!(page.login_hint.as_deref(), Some("alice.example.com"));
        assert_eq!(page.selected_did.as_deref(), Some("did:plc:alice"));
    }

    #[tokio::test]
    async fn accept_requires_signed_in_account() {
        let setup = setup();
        let key = dpop_key();
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let err = accept(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
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
    async fn fragment_response_mode_redirects_in_the_fragment() {
        let setup = setup();
        let key = dpop_key();
        let htu = format!("{ISSUER}/oauth/par");
        let proof = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [proof.as_str()];
        let mut request = par_request(CLIENT_ID);
        request.response_mode = Some("fragment".to_string());
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
        let redirect = setup
            .provider
            .reject(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap();
        let url = Url::parse(&redirect).unwrap();
        // Everything rides in the fragment; the query stays empty. A browser
        // client reading only the fragment must find state and iss there.
        assert!(url.query().is_none(), "query must be empty: {redirect}");
        let fragment = url.fragment().expect("fragment present");
        let pairs: Vec<(String, String)> = url::form_urlencoded::parse(fragment.as_bytes())
            .into_owned()
            .collect();
        assert!(pairs
            .iter()
            .any(|(key, value)| key == "error" && value == "access_denied"));
        assert!(pairs.iter().any(|(key, _)| key == "state"));
        assert!(pairs.iter().any(|(key, _)| key == "iss"));
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
        assert_eq!(err.error_description(), "Invalid token");
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
            &signing_jwk(),
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
            jwt::sign(&header, &claims, &signing_jwk()).unwrap()
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
        assert_eq!(err.error_description(), "Invalid token");

        // the granted scope lives with the stored session, so a session
        // granted without "atproto" is refused whatever the token says
        let stored = setup
            .store
            .read_token(&tokens_jti(&tokens.access_token))
            .await
            .unwrap()
            .unwrap();
        let mut narrow = stored.data.clone();
        narrow.scope = Some("transition:generic".to_string());
        setup
            .store
            .create_token("tok-00000000000000000000000000000000", &narrow, None)
            .await
            .unwrap();
        let err = run_verify(&setup, &key, &token, NOW).await.unwrap_err();
        assert!(err
            .error_description()
            .contains("missing the \"atproto\" scope"));
        // and a stored session whose access has lapsed is reported expired
        // and dropped, even while the token's own exp is in the future
        let mut lapsed = stored.data.clone();
        lapsed.expires_at = NOW - 1;
        setup
            .store
            .delete_token("tok-00000000000000000000000000000000")
            .await
            .unwrap();
        setup
            .store
            .create_token("tok-00000000000000000000000000000000", &lapsed, None)
            .await
            .unwrap();
        let err = run_verify(&setup, &key, &token, NOW).await.unwrap_err();
        assert_eq!(err.error_description(), "Token expired");
        assert!(setup
            .store
            .read_token("tok-00000000000000000000000000000000")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn stored_state_the_provider_cannot_answer_with() {
        let setup = setup();
        // a request whose stored redirect_uri no longer parses
        let mut data = RequestData {
            client_id: CLIENT_ID.to_string(),
            client_auth: ClientAuth::None,
            parameters: AuthorizationRequestParameters {
                client_id: CLIENT_ID.to_string(),
                response_type: "code".to_string(),
                redirect_uri: "not a url".to_string(),
                scope: "atproto".to_string(),
                state: None,
                code_challenge: PKCE_CHALLENGE.to_string(),
                code_challenge_method: CODE_CHALLENGE_METHOD_S256.to_string(),
                login_hint: None,
                response_mode: None,
                prompt: None,
                dpop_jkt: None,
            },
            expires_at: NOW + 300,
            device_id: None,
            did: None,
            code: None,
        };
        let request_id = generate_request_id();
        setup
            .store
            .create_request(&request_id, &data)
            .await
            .unwrap();
        let err = setup
            .provider
            .reject(CLIENT_ID, &request_uri_from_id(&request_id), DEVICE, NOW)
            .await
            .unwrap_err();
        assert!(err.error_description().contains("redirect_uri is invalid"));
        // a session that lost its DPoP binding cannot be turned into a token
        data.parameters.dpop_jkt = None;
        let token = TokenData {
            created_at: NOW,
            updated_at: NOW,
            expires_at: NOW + TOKEN_MAX_AGE,
            client_id: CLIENT_ID.to_string(),
            client_auth: ClientAuth::None,
            device_id: None,
            did: "did:plc:alice".to_string(),
            parameters: data.parameters,
            code: None,
            scope: None,
        };
        let err = setup
            .provider
            .build_token_response("tok-x", &token, None, NOW)
            .unwrap_err();
        assert!(err.error_description().contains("DPoP key binding"));
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
                response_mode: None,
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
            scope: None,
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
        let token = jwt::sign(&header, &claims, &signing_jwk()).unwrap();
        let err = run_verify(&setup, &key, &token, NOW).await.unwrap_err();
        assert_eq!(err.error_description(), "Invalid token");
        assert!(setup
            .store
            .read_token("tok-00000000000000000000000000000000")
            .await
            .unwrap()
            .is_none());
    }

    fn tokens_jti(access_token: &str) -> String {
        jwt::decode(access_token).unwrap().claims.jti.unwrap()
    }

    /// With a shared secret the provider mints HS256 tokens the reference
    /// PDS verifies, publishes no keys, and rejects tokens under another key.
    #[tokio::test]
    async fn symmetric_signing_key_issues_hs256_tokens() {
        let clients = vec![public_metadata(CLIENT_ID)];
        let store = Arc::new(MemoryOAuthStore::new());
        store.add_account(
            account("did:plc:alice", "alice.example.com", None),
            "correct-password",
        );
        let provider = OAuthProvider::new(OAuthProviderConfig {
            issuer: ISSUER.to_string(),
            audience: AUDIENCE.to_string(),
            signing_key: SigningKey::Symmetric(b"a shared secret".to_vec()),
            fetcher: Arc::new(StubFetcher { clients }),
            store: store.clone(),
            dpop: DpopManager::new(None, Box::new(InMemoryReplayStore::default())),
            trusted_clients: vec![CLIENT_ID.to_string()],
            scope_expander: None,
            sessions_since: None,
        });
        let setup = Setup {
            provider,
            store,
            expander: Arc::new(StubExpander::default()),
        };
        assert!(setup.provider.jwks().keys.is_empty());
        let key = dpop_key();
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        let decoded = jwt::decode(&tokens.access_token).unwrap();
        assert_eq!(decoded.header.alg, "HS256");
        assert_eq!(decoded.header.typ.as_deref(), Some(ACCESS_TOKEN_TYP));
        assert!(decoded.claims.extra.get("scope").is_none());
        assert_eq!(decoded.claims.extra["client_id"], json!(CLIENT_ID));
        assert_eq!(tokens.scope, "atproto transition:generic");
        let verified = run_verify(&setup, &key, &tokens.access_token, NOW + 10)
            .await
            .unwrap();
        assert_eq!(verified.did, "did:plc:alice");
        assert_eq!(verified.scopes, ["atproto", "transition:generic"]);
        // a trusted first-party client gets the extended refresh window
        let refreshed = run_refresh(
            &setup,
            CLIENT_ID,
            &key,
            tokens.refresh_token.as_deref().unwrap(),
            NOW + crate::token::PUBLIC_CLIENT_REFRESH_LIFETIME + 1,
        )
        .await
        .unwrap();
        let secret = SigningKey::Symmetric(b"a shared secret".to_vec());
        assert!(jwt::verify_with(&refreshed.access_token, &secret).is_ok());
        let other = SigningKey::Symmetric(b"another secret".to_vec());
        assert!(jwt::verify_with(&refreshed.access_token, &other).is_err());
        let ec = jwt::verify_with(&refreshed.access_token, &signing_key()).unwrap_err();
        assert!(ec.error_description().contains("alg"));
        let mut header = JwtHeader::new("ES256K");
        header.typ = Some(ACCESS_TOKEN_TYP.to_string());
        let mismatch = jwt::sign_with(&header, &JwtClaims::default(), &secret).unwrap_err();
        assert!(mismatch.error_description().contains("alg"));
        assert_eq!(secret.alg().unwrap(), "HS256");
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
                response_mode: None,
                prompt: None,
                dpop_jkt: Some(jkt),
            },
            code: Some(code.clone()),
            scope: None,
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

    #[test]
    fn published_jwks_carries_kid_and_alg() {
        let setup = setup();
        let jwks = setup.provider.jwks();
        let key = &jwks.keys[0];
        assert!(key.d.is_none(), "private scalar must not be published");
        assert!(key.kid.is_some(), "kid required for key rotation");
        assert!(key.alg.is_some(), "alg recommended");
        // kid is the RFC 7638 thumbprint of the public key.
        assert_eq!(key.kid.as_deref(), Some(key.thumbprint().as_str()));
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
            sessions: vec![SessionInfo {
                account: account("did:plc:alice", "alice.example.com", None),
                login_required: false,
                consent_required: true,
            }],
            ..Default::default()
        };
        assert_eq!(page.clone(), page);
        let outcome = AuthorizeOutcome::Page(Box::new(page.clone()));
        assert_eq!(outcome.clone(), outcome);
        assert!(outcome.clone().into_redirect().is_none());
        assert!(AuthorizeOutcome::Redirect("https://x".to_string())
            .into_page()
            .is_none());
        let proof = AccountProof::Device { session_id: "ses" };
        assert_eq!(proof, proof);
        let result = SignInResult {
            account: account("did:plc:alice", "alice.example.com", None),
            ephemeral_token: None,
            new_session_id: None,
        };
        assert_eq!(result.clone(), result);
        let error = ScopeExpandError("boom".to_string());
        assert_eq!(error.to_string(), "boom");
        assert_eq!(error.clone(), error);
        assert!(!format!("{outcome:?}{proof:?}{result:?}{error:?}").is_empty());
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

    fn include_par(request: &mut ParRequest) {
        request.client_id = INCLUDE_CLIENT_ID.to_string();
        request.scope = Some(INCLUDE_SCOPE.to_string());
        request.redirect_uri = Some("https://sets.example.com/callback".to_string());
    }

    #[test]
    fn consent_required_rules() {
        let mut parameters = par_request(CLIENT_ID);
        parameters.scope = Some("atproto transition:generic".to_string());
        let client = Client {
            id: CLIENT_ID.to_string(),
            metadata: public_metadata(CLIENT_ID),
            jwks: None,
        };
        let mut parameters = client.validate_request(&parameters, true).unwrap();
        // no record of a prior grant
        assert!(OAuthProvider::check_consent_required(&parameters, None));
        // superset and exact match are covered
        assert!(!OAuthProvider::check_consent_required(
            &parameters,
            Some("transition:generic atproto transition:email")
        ));
        assert!(!OAuthProvider::check_consent_required(
            &parameters,
            Some("atproto transition:generic")
        ));
        // a subset is not
        assert!(OAuthProvider::check_consent_required(
            &parameters,
            Some("atproto")
        ));
        // an explicit consent prompt always asks
        parameters.prompt = Some("consent".to_string());
        assert!(OAuthProvider::check_consent_required(
            &parameters,
            Some("atproto transition:generic")
        ));
    }

    #[test]
    fn login_required_rules() {
        let setup = setup();
        let linked = |updated_at: u64| DeviceAccount {
            device_id: DEVICE.to_string(),
            account: account("did:plc:alice", "alice.example.com", None),
            device: DeviceData {
                session_id: SESSION.to_string(),
                user_agent: None,
                ip_address: "127.0.0.1".to_string(),
                last_seen_at: NOW,
            },
            created_at: updated_at,
            updated_at,
        };
        assert!(!setup.provider.check_login_required(&linked(NOW), NOW));
        assert!(!setup
            .provider
            .check_login_required(&linked(NOW), NOW + AUTHENTICATION_MAX_AGE));
        assert!(setup
            .provider
            .check_login_required(&linked(NOW), NOW + AUTHENTICATION_MAX_AGE + 1));
        // a clock that went backwards does not demand a login
        assert!(!setup.provider.check_login_required(&linked(NOW + 10), NOW));

        let gated = build_setup(
            vec![public_metadata(CLIENT_ID)],
            SetupOptions {
                sessions_since: Some(NOW - 100),
                ..Default::default()
            },
        );
        assert!(gated.provider.check_login_required(&linked(NOW - 101), NOW));
        assert!(!gated.provider.check_login_required(&linked(NOW - 100), NOW));
    }

    #[tokio::test]
    async fn ephemeral_tokens_are_bound_and_expire() {
        let setup = setup();
        let request_uri = "urn:ietf:params:oauth:request_uri:req-00000000000000000000000000000000";
        let token = setup
            .provider
            .create_ephemeral_token("did:plc:alice", DEVICE, request_uri, NOW)
            .unwrap();
        let decoded = jwt::decode(&token).unwrap();
        assert_eq!(decoded.header.typ.as_deref(), Some(ACCESS_TOKEN_TYP));
        assert_eq!(
            decoded.claims.aud,
            Some(json!(format!("oauth-provider-api@{ISSUER}")))
        );
        assert_eq!(decoded.claims.exp, Some(NOW + EPHEMERAL_SESSION_MAX_AGE));
        setup
            .provider
            .verify_ephemeral_token(&token, "did:plc:alice", DEVICE, request_uri, NOW + 10)
            .unwrap();
        // bound to the account, the device and the request
        for (did, device, uri) in [
            ("did:plc:bob", DEVICE, request_uri),
            ("did:plc:alice", "dev-other", request_uri),
            (
                "did:plc:alice",
                DEVICE,
                "urn:ietf:params:oauth:request_uri:req-11111111111111111111111111111111",
            ),
        ] {
            let err = setup
                .provider
                .verify_ephemeral_token(&token, did, device, uri, NOW + 10)
                .unwrap_err();
            assert!(err.error_description().contains("does not match"));
        }
        // and to its lifetime
        let err = setup
            .provider
            .verify_ephemeral_token(
                &token,
                "did:plc:alice",
                DEVICE,
                request_uri,
                NOW + EPHEMERAL_SESSION_MAX_AGE,
            )
            .unwrap_err();
        assert!(err.error_description().contains("expired"));
        // an access token is not a sign-in proof even though it is signed
        // by the same key
        let key = dpop_key();
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        let err = setup
            .provider
            .verify_ephemeral_token(
                &tokens.access_token,
                "did:plc:alice",
                DEVICE,
                request_uri,
                NOW,
            )
            .unwrap_err();
        assert!(err.error_description().contains("aud"));
        assert!(setup
            .provider
            .verify_ephemeral_token("garbage", "did:plc:alice", DEVICE, request_uri, NOW)
            .is_err());
    }

    #[tokio::test]
    async fn sign_in_remembered_or_ephemeral() {
        let setup = setup();
        let key = dpop_key();

        // remembered: the device session rotates and the account is linked
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let result = sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "alice.example.com",
            "correct-password",
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(result.account.did, "did:plc:alice");
        assert!(result.ephemeral_token.is_none());
        let new_session = result.new_session_id.clone().unwrap();
        assert_ne!(new_session, SESSION);
        assert_eq!(device_session(&setup).await, new_session);
        let linked = setup
            .store
            .get_device_account(DEVICE, "did:plc:alice")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(linked.updated_at, NOW);

        // a stale presented session id is refused and changes nothing
        let err = setup
            .provider
            .sign_in(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "bob.example.com",
                "bobs-password",
                true,
                SESSION,
                NOW + 1,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("session changed"));
        assert!(setup
            .store
            .get_device_account(DEVICE, "did:plc:bob")
            .await
            .unwrap()
            .is_none());
        assert_eq!(device_session(&setup).await, new_session);

        // not remembered: the link is dropped and a proof is issued instead
        let result = setup
            .provider
            .sign_in(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "alice.example.com",
                "correct-password",
                false,
                &new_session,
                NOW,
            )
            .await
            .unwrap();
        assert!(result.new_session_id.is_none());
        assert!(setup
            .store
            .get_device_account(DEVICE, "did:plc:alice")
            .await
            .unwrap()
            .is_none());
        let proof = result.ephemeral_token.unwrap();
        // the device session alone can no longer accept
        let err = accept(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap_err();
        assert!(err.error_description().contains("not signed in"));
        // the proof is bound to this request
        let other = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let err = setup
            .provider
            .accept(
                CLIENT_ID,
                &other.request_uri,
                DEVICE,
                "did:plc:alice",
                AccountProof::Ephemeral(&proof),
                None,
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("does not match"));
        let redirect = setup
            .provider
            .accept(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "did:plc:alice",
                AccountProof::Ephemeral(&proof),
                None,
                NOW,
            )
            .await
            .unwrap();
        assert!(query_value(&Url::parse(&redirect).unwrap(), "code").is_some());
    }

    #[tokio::test]
    async fn accept_refuses_stale_and_expired_device_sessions() {
        let setup = setup();
        let key = dpop_key();
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let before = device_session(&setup).await;
        sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "alice.example.com",
            "correct-password",
            NOW,
        )
        .await
        .unwrap();
        // a request that validated the pre-login secret is refused even
        // though the membership row now exists
        let err = setup
            .provider
            .accept(
                CLIENT_ID,
                &par.request_uri,
                DEVICE,
                "did:plc:alice",
                AccountProof::Device {
                    session_id: &before,
                },
                None,
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.error_description().contains("not signed in"));

        // an authentication older than the maximum age must sign in again
        setup
            .store
            .upsert_device_account(DEVICE, "did:plc:alice", NOW - AUTHENTICATION_MAX_AGE - 1)
            .await
            .unwrap();
        let err = accept(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap_err();
        assert_eq!(
            err,
            OAuthError::InvalidRequest("login required".to_string())
        );
        assert!(setup
            .store
            .read_request(request_id_from_uri(&par.request_uri).unwrap())
            .await
            .unwrap()
            .unwrap()
            .code
            .is_none());

        // so must one that predates the enforcement cutoff
        let gated = build_setup(
            vec![public_metadata(CLIENT_ID)],
            SetupOptions {
                sessions_since: Some(NOW - 100),
                ..Default::default()
            },
        );
        let par = run_par(&gated, CLIENT_ID, &key, NOW).await;
        device_session(&gated).await;
        gated
            .store
            .upsert_device_account(DEVICE, "did:plc:alice", NOW - 200)
            .await
            .unwrap();
        let err = accept(
            &gated,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap_err();
        assert_eq!(
            err,
            OAuthError::InvalidRequest("login required".to_string())
        );
        gated
            .store
            .upsert_device_account(DEVICE, "did:plc:alice", NOW - 50)
            .await
            .unwrap();
        accept(
            &gated,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn authorize_decision_table() {
        let setup = setup_trusted();
        let key = dpop_key();
        let request_id =
            |par: &ParResponse| request_id_from_uri(&par.request_uri).unwrap().to_string();

        // prompt=none without a session: login_required, request discarded
        let par = run_par_with(&setup, &key, NOW, |r| r.prompt = Some("none".to_string())).await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(
            query_value(&url, "error").as_deref(),
            Some("login_required")
        );
        assert_eq!(query_value(&url, "state").as_deref(), Some("state-123"));
        assert!(setup
            .store
            .read_request(&request_id(&par))
            .await
            .unwrap()
            .is_none());

        // select_account without a session: account_selection_required
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.prompt = Some("select_account".to_string())
        })
        .await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(
            query_value(&url, "error").as_deref(),
            Some("account_selection_required")
        );

        // sign alice in; prompt=none without a hint names no session
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "alice.example.com",
            "correct-password",
            NOW,
        )
        .await
        .unwrap();
        let par = run_par_with(&setup, &key, NOW, |r| r.prompt = Some("none".to_string())).await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(
            query_value(&url, "error").as_deref(),
            Some("login_required")
        );

        // with the hint, nothing authorized yet, so prompt=none needs consent
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.prompt = Some("none".to_string());
            r.login_hint = Some("alice.example.com".to_string());
        })
        .await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(
            query_value(&url, "error").as_deref(),
            Some("consent_required")
        );

        // no prompt, no hint: the page, nothing preselected, consent required
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        let page = expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert!(page.prompt.is_none());
        assert!(page.selected_did.is_none());
        assert_eq!(page.sessions.len(), 1);
        assert!(page.sessions[0].consent_required);
        assert!(!page.sessions[0].login_required);

        // a hint naming a session that still needs consent: preselected
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.login_hint = Some("did:plc:alice".to_string())
        })
        .await;
        let page = expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(page.selected_did.as_deref(), Some("did:plc:alice"));

        // a previous grant covering the request: prompt=none issues a code
        setup
            .store
            .set_authorized_client(
                "did:plc:alice",
                CLIENT_ID,
                "atproto transition:generic transition:email",
            )
            .await
            .unwrap();
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.prompt = Some("none".to_string());
            r.login_hint = Some("alice.example.com".to_string());
        })
        .await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        let code = query_value(&url, "code").unwrap();
        run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();

        // a login_hint naming the one fresh, consented session: silent code
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.login_hint = Some("Alice.Example.Com".to_string())
        })
        .await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert!(query_value(&url, "code").is_some());

        // a hint for an account that is not signed in: the page, nothing selected
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.login_hint = Some("bob.example.com".to_string())
        })
        .await;
        let page = expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert!(page.selected_did.is_none());

        // prompt=consent keeps the page even though the grant covers it
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.prompt = Some("consent".to_string());
            r.login_hint = Some("alice.example.com".to_string());
        })
        .await;
        let page = expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert!(page.sessions[0].consent_required);
        assert_eq!(page.selected_did.as_deref(), Some("did:plc:alice"));

        // prompt=login marks every session as needing a login
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.prompt = Some("login".to_string());
            r.login_hint = Some("alice.example.com".to_string());
        })
        .await;
        let page = expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert!(page.sessions[0].login_required);
        assert_eq!(page.selected_did.as_deref(), Some("did:plc:alice"));

        // select_account with sessions: the page with no preselection
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.prompt = Some("select_account".to_string())
        })
        .await;
        let page = expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert!(page.selected_did.is_none());
        assert_eq!(page.sessions.len(), 1);

        // a stale authentication: prompt=none answers login_required
        setup
            .store
            .upsert_device_account(DEVICE, "did:plc:alice", NOW - AUTHENTICATION_MAX_AGE - 1)
            .await
            .unwrap();
        let par = run_par_with(&setup, &key, NOW, |r| r.prompt = Some("none".to_string())).await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(
            query_value(&url, "error").as_deref(),
            Some("login_required")
        );
        // and the hinted silent path is not taken either
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.login_hint = Some("alice.example.com".to_string())
        })
        .await;
        expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        setup
            .store
            .upsert_device_account(DEVICE, "did:plc:alice", NOW)
            .await
            .unwrap();

        // two sessions matching the hint (a handle that moved between
        // accounts): prompt=none cannot pick one
        setup.store.add_account(
            account("did:plc:alice2", "alice.example.com", None),
            "second-password",
        );
        setup
            .store
            .upsert_device_account(DEVICE, "did:plc:alice2", NOW)
            .await
            .unwrap();
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.prompt = Some("none".to_string());
            r.login_hint = Some("alice.example.com".to_string());
        })
        .await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(
            query_value(&url, "error").as_deref(),
            Some("account_selection_required")
        );

        // a deactivated account never qualifies for single sign-on
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "carol.example.com",
            "carols-password",
            NOW,
        )
        .await
        .unwrap();
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.prompt = Some("none".to_string());
            r.login_hint = Some("carol.example.com".to_string());
        })
        .await;
        let url = expect_redirect(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(
            query_value(&url, "error").as_deref(),
            Some("login_required")
        );
        // the page still lists the deactivated session for reactivation
        let par = run_par_with(&setup, &key, NOW, |r| {
            r.login_hint = Some("carol.example.com".to_string())
        })
        .await;
        let page = expect_page(
            setup
                .provider
                .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(page.selected_did.as_deref(), Some("did:plc:carol"));
        assert_eq!(page.sessions.len(), 3);
    }

    #[tokio::test]
    async fn accept_records_consent_only_when_asked_and_narrows_scope() {
        let setup = setup_trusted();
        let key = dpop_key();
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        sign_in(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "alice.example.com",
            "correct-password",
            NOW,
        )
        .await
        .unwrap();
        // a grant outside the request, or without atproto, is refused
        for granted in ["atproto transition:email", "transition:generic"] {
            let err = accept(
                &setup,
                CLIENT_ID,
                &par.request_uri,
                "did:plc:alice",
                Some(granted),
                NOW,
            )
            .await
            .unwrap_err();
            assert!(err.error_description().contains("granted scope"));
        }
        // the user drops a scope: consent is recorded for what was granted
        let redirect = accept(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            Some("atproto"),
            NOW,
        )
        .await
        .unwrap();
        let code = query_value(&Url::parse(&redirect).unwrap(), "code").unwrap();
        let tokens = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        assert_eq!(tokens.scope, "atproto");
        assert_eq!(
            setup
                .store
                .get_authorized_client_scope("did:plc:alice", CLIENT_ID)
                .await
                .unwrap()
                .as_deref(),
            Some("atproto")
        );
        // the next full grant unions into the record
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        setup
            .provider
            .authorize(CLIENT_ID, &par.request_uri, DEVICE, NOW)
            .await
            .unwrap();
        accept(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(
            setup
                .store
                .get_authorized_client_scope("did:plc:alice", CLIENT_ID)
                .await
                .unwrap()
                .as_deref(),
            Some("atproto transition:generic")
        );
        // a covered request accepted again leaves the record alone
        setup
            .store
            .set_authorized_client(
                "did:plc:alice",
                CLIENT_ID,
                "atproto transition:generic extra",
            )
            .await
            .unwrap();
        let par = run_par(&setup, CLIENT_ID, &key, NOW).await;
        accept(
            &setup,
            CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(
            setup
                .store
                .get_authorized_client_scope("did:plc:alice", CLIENT_ID)
                .await
                .unwrap()
                .as_deref(),
            Some("atproto transition:generic extra")
        );
    }

    #[tokio::test]
    async fn permission_set_failures_fail_closed_except_at_refresh() {
        init_tracing();
        let setup = setup_trusted();
        let key = dpop_key();
        let htu = format!("{ISSUER}/oauth/par");

        // at PAR: no request is stored
        setup.expander.fail.store(true, Ordering::SeqCst);
        let proof_jwt = proof(&key, "POST", &htu, NOW, None, None);
        let headers = [proof_jwt.as_str()];
        let mut request = par_request(CLIENT_ID);
        include_par(&mut request);
        let err = setup
            .provider
            .pushed_authorization_request(
                &credentials(INCLUDE_CLIENT_ID),
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
        assert_eq!(err.error_code(), "invalid_scope");
        assert_eq!(
            err.error_description(),
            "Unable to retrieve permission sets"
        );
        setup.expander.fail.store(false, Ordering::SeqCst);

        // on the silent path: the client gets invalid_scope, no code
        let par = run_par_with(&setup, &key, NOW, include_par).await;
        sign_in(
            &setup,
            INCLUDE_CLIENT_ID,
            &par.request_uri,
            "alice.example.com",
            "correct-password",
            NOW,
        )
        .await
        .unwrap();
        setup
            .store
            .set_authorized_client("did:plc:alice", INCLUDE_CLIENT_ID, INCLUDE_SCOPE)
            .await
            .unwrap();
        let par = run_par_with(&setup, &key, NOW, |r| {
            include_par(r);
            r.prompt = Some("none".to_string());
            r.login_hint = Some("alice.example.com".to_string());
        })
        .await;
        setup.expander.fail.store(true, Ordering::SeqCst);
        let url = expect_redirect(
            setup
                .provider
                .authorize(INCLUDE_CLIENT_ID, &par.request_uri, DEVICE, NOW)
                .await
                .unwrap(),
        );
        assert_eq!(query_value(&url, "error").as_deref(), Some("invalid_scope"));
        assert!(query_value(&url, "code").is_none());
        assert!(setup
            .store
            .read_request(request_id_from_uri(&par.request_uri).unwrap())
            .await
            .unwrap()
            .is_none());

        // on acceptance: the same error redirect
        setup.expander.fail.store(false, Ordering::SeqCst);
        let par = run_par_with(&setup, &key, NOW, |r| {
            include_par(r);
            r.prompt = Some("consent".to_string());
        })
        .await;
        setup.expander.fail.store(true, Ordering::SeqCst);
        let redirect = accept(
            &setup,
            INCLUDE_CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap();
        let url = Url::parse(&redirect).unwrap();
        assert_eq!(query_value(&url, "error").as_deref(), Some("invalid_scope"));
        assert!(query_value(&url, "code").is_none());
        setup.expander.fail.store(false, Ordering::SeqCst);

        // at the token endpoint: no token row is created
        let par = run_par_with(&setup, &key, NOW, include_par).await;
        let redirect = accept(
            &setup,
            INCLUDE_CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap();
        let code = query_value(&Url::parse(&redirect).unwrap(), "code").unwrap();
        setup.expander.fail.store(true, Ordering::SeqCst);
        let err = run_token(&setup, INCLUDE_CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap_err();
        assert_eq!(err.error_code(), "invalid_scope");
        assert!(setup
            .store
            .list_account_tokens("did:plc:alice")
            .await
            .unwrap()
            .is_empty());
        setup.expander.fail.store(false, Ordering::SeqCst);

        // issuance stores the compiled grants, without the include token
        let par = run_par_with(&setup, &key, NOW, include_par).await;
        let redirect = accept(
            &setup,
            INCLUDE_CLIENT_ID,
            &par.request_uri,
            "did:plc:alice",
            None,
            NOW,
        )
        .await
        .unwrap();
        let code = query_value(&Url::parse(&redirect).unwrap(), "code").unwrap();
        let tokens = run_token(&setup, INCLUDE_CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        assert_eq!(
            tokens.scope,
            format!("atproto transition:generic {COMPILED_SET}")
        );
        let stored = setup
            .store
            .read_token(&tokens_jti(&tokens.access_token))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.data.scope.as_deref(), Some(tokens.scope.as_str()));
        assert_eq!(stored.data.parameters.scope, INCLUDE_SCOPE);

        // a resolver outage at refresh keeps the old grants and the session
        setup.expander.fail.store(true, Ordering::SeqCst);
        let refreshed = run_refresh(
            &setup,
            INCLUDE_CLIENT_ID,
            &key,
            tokens.refresh_token.as_deref().unwrap(),
            NOW + 10,
        )
        .await
        .unwrap();
        assert_eq!(refreshed.scope, tokens.scope);
        setup.expander.fail.store(false, Ordering::SeqCst);

        // a changed set takes effect at the next refresh
        setup.expander.changed.store(true, Ordering::SeqCst);
        let refreshed = run_refresh(
            &setup,
            INCLUDE_CLIENT_ID,
            &key,
            refreshed.refresh_token.as_deref().unwrap(),
            NOW + 20,
        )
        .await
        .unwrap();
        assert_eq!(
            refreshed.scope,
            format!("atproto transition:generic {RECOMPILED_SET}")
        );
        let stored = setup
            .store
            .read_token(&tokens_jti(&refreshed.access_token))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.data.scope.as_deref(), Some(refreshed.scope.as_str()));
        let access = run_verify(&setup, &key, &refreshed.access_token, NOW + 20)
            .await
            .unwrap();
        assert!(access.scopes.iter().any(|scope| scope == RECOMPILED_SET));
    }

    #[tokio::test]
    async fn account_sessions_listing_and_revocation() {
        init_tracing();
        let mut no_refresh = public_metadata(OTHER_CLIENT_ID);
        no_refresh.grant_types = vec![GRANT_AUTHORIZATION_CODE.to_string()];
        let setup = build_setup(
            vec![public_metadata(CLIENT_ID), no_refresh],
            SetupOptions {
                trusted: vec![CLIENT_ID.to_string()],
                ..Default::default()
            },
        );
        let key = dpop_key();
        let code = run_authorization(&setup, CLIENT_ID, &key, NOW).await;
        let trusted = run_token(&setup, CLIENT_ID, &key, &code, NOW)
            .await
            .unwrap();
        let code = run_authorization(&setup, OTHER_CLIENT_ID, &key, NOW + 1).await;
        let one_shot = run_token(&setup, OTHER_CLIENT_ID, &key, &code, NOW + 1)
            .await
            .unwrap();
        assert!(one_shot.refresh_token.is_none());
        // a session for a client whose metadata can no longer be fetched
        let mut orphan = setup
            .store
            .read_token(&tokens_jti(&trusted.access_token))
            .await
            .unwrap()
            .unwrap()
            .data;
        orphan.client_id = "https://gone.example.com/client.json".to_string();
        orphan.created_at = NOW + 2;
        setup
            .store
            .create_token("tok-orphan", &orphan, Some("ref-orphan"))
            .await
            .unwrap();

        let sessions = setup
            .provider
            .list_account_sessions("did:plc:alice", NOW + 3)
            .await
            .unwrap();
        let ids: Vec<&str> = sessions.iter().map(|s| s.token_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "tok-orphan",
                tokens_jti(&one_shot.access_token).as_str(),
                tokens_jti(&trusted.access_token).as_str(),
            ]
        );
        assert!(sessions.iter().all(|s| s.active));
        assert!(sessions[0].client_metadata.is_none());
        assert_eq!(
            sessions[2].client_metadata.as_ref().unwrap().client_id,
            CLIENT_ID
        );
        assert_eq!(
            sessions[2].scope.as_deref(),
            Some("atproto transition:generic")
        );
        assert_eq!(sessions[2].created_at, NOW);
        assert_eq!(sessions[2].updated_at, NOW);
        assert!(setup
            .provider
            .list_account_sessions("did:plc:bob", NOW)
            .await
            .unwrap()
            .is_empty());

        // after the access token lapses, only refreshable sessions remain,
        // shown as inactive
        let later = NOW + TOKEN_MAX_AGE + 10;
        let sessions = setup
            .provider
            .list_account_sessions("did:plc:alice", later)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|s| !s.active));
        assert!(sessions
            .iter()
            .all(|s| s.token_id != tokens_jti(&one_shot.access_token)));

        // past the refresh lifetime everything is gone (the orphan is held to
        // the public-client lifetime, the trusted client to the longer one)
        let sessions = setup
            .provider
            .list_account_sessions(
                "did:plc:alice",
                NOW + crate::token::PUBLIC_CLIENT_SESSION_LIFETIME + 10,
            )
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].client_id, CLIENT_ID);

        // revocation is limited to the account's own tokens
        let trusted_id = tokens_jti(&trusted.access_token);
        let err = setup
            .provider
            .revoke_account_token("did:plc:bob", &trusted_id)
            .await
            .unwrap_err();
        assert_eq!(err.error_description(), "Invalid token");
        let err = setup
            .provider
            .revoke_account_token("did:plc:alice", "tok-unknown")
            .await
            .unwrap_err();
        assert_eq!(err.error_description(), "Invalid token");
        setup
            .provider
            .revoke_account_token("did:plc:alice", &trusted_id)
            .await
            .unwrap();
        assert!(setup.store.read_token(&trusted_id).await.unwrap().is_none());
    }
}
