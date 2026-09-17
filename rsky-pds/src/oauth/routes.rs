use super::body::OAuthBody;
use super::{
    csrf_token, device_cookie, ensure_device_session, now_secs, DeviceSession, SharedOAuthProvider,
};
use crate::metrics::{record_login_success, record_oauth_authorization_granted};
use crate::oauth_scope::INCLUDE_PREFIX;
use crate::permission_set::SharedPermissionSets;
use crate::ui::client::client_view;
use crate::ui::pages::oauth::{ConsentPage, ErrorPage, SignInPage, SignInView};
use crate::ui::pages::AccountCardView;
use crate::ui::respond::{render_page, UiHtml};
use crate::ui::scopes::{permission_groups, IncludeSetView};
use crate::ui::shell::PageShell;
use crate::ui::technical::technical_items;
use crate::ui::UiState;
use rocket::form::Form;
use rocket::http::{ContentType, CookieJar, Header, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::{Redirect, Responder, Response};
use rocket::serde::json::Json;
use rocket::FromForm;
use rocket::State;
use rsky_common::env::env_str;
use rsky_oauth::client::ParRequest;
use rsky_oauth::dpop::DpopRequest;
use rsky_oauth::store::AccountInfo;
use rsky_oauth::types::GRANT_AUTHORIZATION_CODE;
use rsky_oauth::{
    AccountProof, AuthorizeOutcome, AuthorizePageData, ClientCredentials, OAuthError, TokenRequest,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Cursor;

/// Request material needed to validate DPoP proofs.
pub struct OAuthRequestInfo {
    pub method: String,
    pub uri: String,
    pub dpop_headers: Vec<String>,
    pub user_agent: Option<String>,
    pub ip_address: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OAuthRequestInfo {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Some(cfg) = req.rocket().state::<crate::config::ServerConfig>() else {
            return Outcome::Error((Status::InternalServerError, ()));
        };
        Outcome::Success(OAuthRequestInfo {
            method: req.method().as_str().to_string(),
            uri: format!("{}{}", cfg.service.public_url, req.uri()),
            dpop_headers: req.headers().get("dpop").map(String::from).collect(),
            user_agent: req.headers().get_one("user-agent").map(String::from),
            ip_address: req
                .client_ip()
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        })
    }
}

impl OAuthRequestInfo {
    fn dpop_request<'a>(
        &'a self,
        headers: &'a [&'a str],
        access_token: Option<&'a str>,
    ) -> DpopRequest<'a> {
        DpopRequest {
            method: &self.method,
            uri: &self.uri,
            dpop_headers: headers,
            access_token,
        }
    }
}

/// JSON responder for the PAR/token/revoke endpoints: emits the
/// DPoP-Nonce header and RFC 6749 cache directives.
pub struct OAuthApiResponse {
    status: Status,
    body: Value,
    dpop_nonce: Option<String>,
}

impl OAuthApiResponse {
    fn ok(status: Status, body: Value, dpop_nonce: Option<String>) -> Self {
        Self {
            status,
            body,
            dpop_nonce,
        }
    }

    fn error(error: OAuthError, dpop_nonce: Option<String>) -> Self {
        Self {
            status: Status::new(error.status()),
            body: error.to_json(),
            dpop_nonce,
        }
    }
}

impl<'r> Responder<'r, 'static> for OAuthApiResponse {
    fn respond_to(self, _request: &'r Request<'_>) -> rocket::response::Result<'static> {
        let body = self.body.to_string();
        let mut response = Response::build();
        response
            .status(self.status)
            .header(ContentType::JSON)
            .header(Header::new("Cache-Control", "no-store"))
            .header(Header::new("Pragma", "no-cache"))
            .sized_body(body.len(), Cursor::new(body));
        if let Some(nonce) = self.dpop_nonce {
            response.header(Header::new("DPoP-Nonce", nonce));
            response.header(Header::new(
                "Access-Control-Expose-Headers",
                "DPoP-Nonce, WWW-Authenticate",
            ));
        }
        Ok(response.finalize())
    }
}

type HtmlPage = UiHtml;

fn render_error(shell: &PageShell, status: Status, message: impl Into<String>) -> HtmlPage {
    render_page(status, shell, &ErrorPage::new(shell.clone(), message))
}

fn oauth_error_page(shell: &PageShell, error: OAuthError) -> HtmlPage {
    render_error(
        shell,
        Status::new(error.status()),
        error.error_description(),
    )
}

#[derive(FromForm, serde::Deserialize)]
pub struct ParFormData {
    pub client_id: Option<String>,
    pub response_type: Option<String>,
    pub redirect_uri: Option<String>,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub login_hint: Option<String>,
    pub response_mode: Option<String>,
    pub prompt: Option<String>,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
}

impl ParFormData {
    fn credentials(&self) -> ClientCredentials {
        ClientCredentials {
            client_id: self.client_id.clone().unwrap_or_default(),
            client_assertion_type: self.client_assertion_type.clone(),
            client_assertion: self.client_assertion.clone(),
        }
    }

    fn par_request(&self) -> ParRequest {
        ParRequest {
            client_id: self.client_id.clone().unwrap_or_default(),
            response_type: self.response_type.clone().unwrap_or_default(),
            redirect_uri: self.redirect_uri.clone(),
            scope: self.scope.clone(),
            state: self.state.clone(),
            code_challenge: self.code_challenge.clone(),
            code_challenge_method: self.code_challenge_method.clone(),
            login_hint: self.login_hint.clone(),
            response_mode: self.response_mode.clone(),
            prompt: self.prompt.clone(),
        }
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/par", data = "<form>")]
pub async fn oauth_par(
    form: OAuthBody<ParFormData>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
) -> OAuthApiResponse {
    let provider = &shared.provider;
    let now = now_secs();
    let nonce = provider.next_dpop_nonce(now);
    let headers: Vec<&str> = info.dpop_headers.iter().map(String::as_str).collect();
    match provider
        .pushed_authorization_request(
            &form.credentials(),
            &form.par_request(),
            &info.dpop_request(&headers, None),
            now,
        )
        .await
    {
        Ok(response) => OAuthApiResponse::ok(
            Status::Created,
            serde_json::to_value(response).expect("PAR response serialization cannot fail"),
            nonce,
        ),
        Err(error) => OAuthApiResponse::error(error, nonce),
    }
}

/// Only the initial `authorization_code` exchange mints a new session; a
/// `refresh_token` grant renews an existing one and isn't a fresh "session
/// created" event -- pulled out as its own function so this distinction is
/// unit-tested directly rather than only provable by an exact metric count
/// (the Prometheus recorder is a single process-wide instance, so
/// integration tests that race concurrently in the same test binary can't
/// safely assert on it).
fn is_new_oauth_session(grant_type: &str) -> bool {
    grant_type == GRANT_AUTHORIZATION_CODE
}

#[derive(FromForm, serde::Deserialize)]
pub struct TokenFormData {
    pub grant_type: Option<String>,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/token", data = "<form>")]
pub async fn oauth_token(
    form: OAuthBody<TokenFormData>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
) -> OAuthApiResponse {
    let provider = &shared.provider;
    let now = now_secs();
    let nonce = provider.next_dpop_nonce(now);
    let headers: Vec<&str> = info.dpop_headers.iter().map(String::as_str).collect();
    let credentials = ClientCredentials {
        client_id: form.client_id.clone().unwrap_or_default(),
        client_assertion_type: form.client_assertion_type.clone(),
        client_assertion: form.client_assertion.clone(),
    };
    let request = TokenRequest {
        grant_type: form.grant_type.clone().unwrap_or_default(),
        code: form.code.clone(),
        redirect_uri: form.redirect_uri.clone(),
        code_verifier: form.code_verifier.clone(),
        refresh_token: form.refresh_token.clone(),
    };
    let is_new_session = is_new_oauth_session(&request.grant_type);
    match provider
        .token(
            &credentials,
            &request,
            &info.dpop_request(&headers, None),
            now,
        )
        .await
    {
        Ok(response) => {
            if is_new_session {
                record_login_success("oauth");
            }
            OAuthApiResponse::ok(
                Status::Ok,
                serde_json::to_value(response).expect("token response serialization cannot fail"),
                nonce,
            )
        }
        Err(error) => OAuthApiResponse::error(error, nonce),
    }
}

#[derive(FromForm, serde::Deserialize)]
pub struct RevokeFormData {
    pub token: Option<String>,
    pub client_id: Option<String>,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/revoke", data = "<form>")]
pub async fn oauth_revoke(
    form: OAuthBody<RevokeFormData>,
    shared: &State<SharedOAuthProvider>,
) -> OAuthApiResponse {
    let provider = &shared.provider;
    let now = now_secs();
    let nonce = provider.next_dpop_nonce(now);
    let credentials = ClientCredentials {
        client_id: form.client_id.clone().unwrap_or_default(),
        client_assertion_type: form.client_assertion_type.clone(),
        client_assertion: form.client_assertion.clone(),
    };
    let Some(token) = form.token.clone() else {
        return OAuthApiResponse::error(
            OAuthError::InvalidRequest("token is required".to_string()),
            nonce,
        );
    };
    match provider.revoke(&credentials, &token, now).await {
        Ok(()) => OAuthApiResponse::ok(Status::Ok, serde_json::json!({}), nonce),
        Err(error) => OAuthApiResponse::error(error, nonce),
    }
}

#[tracing::instrument(skip_all)]
#[rocket::get("/oauth/jwks")]
pub async fn oauth_jwks(shared: &State<SharedOAuthProvider>) -> Json<rsky_oauth::JwkSet> {
    Json(shared.provider.jwks())
}

#[tracing::instrument(skip_all)]
#[rocket::get("/.well-known/oauth-authorization-server")]
pub async fn oauth_authorization_server_metadata(
    shared: &State<SharedOAuthProvider>,
) -> Json<Value> {
    Json(shared.provider.authorization_server_metadata())
}

#[tracing::instrument(skip_all)]
#[rocket::get("/.well-known/oauth-protected-resource")]
pub async fn oauth_protected_resource_metadata(shared: &State<SharedOAuthProvider>) -> Json<Value> {
    Json(shared.provider.protected_resource_metadata())
}

const SIGN_IN_ACTION: &str = "/oauth/authorize/sign-in";
const SELECT_ACTION: &str = "/oauth/authorize/select";
const ACCEPT_ACTION: &str = "/oauth/authorize/accept";
const REJECT_ACTION: &str = "/oauth/authorize/reject";
const CREDENTIALS_REJECTED: &str = "Invalid identifier or password";
const CODE_REJECTED: &str = "The sign-in code was not accepted";
const PERMISSION_SETS_UNAVAILABLE: &str = "Unable to retrieve permission sets";

fn authorize_href(client_id: &str, request_uri: &str, view: Option<&str>) -> String {
    let mut pairs = vec![("client_id", client_id), ("request_uri", request_uri)];
    if let Some(view) = view {
        pairs.push(("view", view));
    }
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish();
    format!("/oauth/authorize?{query}")
}

/// What the sign-in page is asked to show on top of the request itself.
#[derive(Default)]
struct SignInOptions {
    view: Option<SignInView>,
    identifier: Option<String>,
    error: Option<String>,
    otp_hint: Option<String>,
    otp_error: bool,
}

fn sign_in_page(
    shell: &PageShell,
    page: &AuthorizePageData,
    session: &DeviceSession,
    options: SignInOptions,
) -> SignInPage {
    let hinted = page.login_hint.clone().filter(|hint| !hint.is_empty());
    let view = options.view.unwrap_or(if hinted.is_some() {
        SignInView::ForcedIdentifier
    } else if page.sessions.is_empty() {
        SignInView::Form
    } else {
        SignInView::Picker
    });
    let identifier_readonly = matches!(
        view,
        SignInView::ForcedIdentifier | SignInView::ConfirmSelected
    );
    let identifier = options.identifier.or(hinted).unwrap_or_default();
    let back_href = if view == SignInView::Picker || page.sessions.is_empty() {
        String::new()
    } else {
        authorize_href(&page.client_id, &page.request_uri, None)
    };
    SignInPage {
        shell: shell.clone(),
        view,
        subtitle: SignInPage::subtitle_for(view).to_string(),
        client: client_view(page),
        client_id: page.client_id.clone(),
        request_uri: page.request_uri.clone(),
        csrf: session.csrf.clone(),
        identifier,
        identifier_readonly,
        error: options.error,
        otp_hint: options.otp_hint,
        otp_error: options.otp_error,
        show_remember: false,
        remember_checked: false,
        submit_label: "Sign in".to_string(),
        sessions: page
            .sessions
            .iter()
            .map(|info| AccountCardView::from_account(&info.account, info.login_required))
            .collect(),
        show_picker: view == SignInView::Picker,
        sign_in_action: SIGN_IN_ACTION.to_string(),
        select_action: SELECT_ACTION.to_string(),
        another_account_href: authorize_href(&page.client_id, &page.request_uri, Some("sign-in")),
        signup_href: env_str("PDS_OAUTH_SIGNUP_URL").filter(|url| !url.is_empty()),
        forgot_href: None,
        back_href,
        back_label: "Back".to_string(),
    }
}

/// Every `include:` set the request names, resolved so the page shows what
/// each one confers. A set that cannot be resolved fails the page, as it
/// would fail the grant.
async fn include_sets(
    sets: &SharedPermissionSets,
    scopes: &[String],
) -> Result<BTreeMap<String, IncludeSetView>, String> {
    let mut views = BTreeMap::new();
    for nsid in scopes.iter().filter_map(|s| s.strip_prefix(INCLUDE_PREFIX)) {
        if views.contains_key(nsid) {
            continue;
        }
        let scopes = sets
            .resolver
            .try_resolved_scopes(nsid)
            .await
            .map_err(|error| {
                tracing::warn!(%error, nsid, "permission set could not be resolved for consent");
                PERMISSION_SETS_UNAVAILABLE.to_string()
            })?;
        views.insert(
            nsid.to_string(),
            IncludeSetView {
                title: None,
                detail: None,
                scopes,
            },
        );
    }
    Ok(views)
}

async fn consent_page(
    ui: &UiState,
    sets: &SharedPermissionSets,
    page: &AuthorizePageData,
    session: &DeviceSession,
    account: &AccountInfo,
) -> Result<ConsentPage, HtmlPage> {
    let shell = &ui.shell;
    let include_sets = include_sets(sets, &page.scopes)
        .await
        .map_err(|message| render_error(shell, Status::BadRequest, message))?;
    let first_party = page.client_trusted && ui.is_first_party_client(&page.client_id);
    let grouping = permission_groups(
        &page.scopes,
        &include_sets,
        first_party,
        shell.app_name.as_deref(),
    );
    Ok(ConsentPage {
        shell: (**shell).clone(),
        client: client_view(page),
        client_id: page.client_id.clone(),
        request_uri: page.request_uri.clone(),
        csrf: session.csrf.clone(),
        account: AccountCardView::from_account(account, false),
        scope_raw: page.scopes.join(" "),
        only_atproto: grouping.only_atproto,
        identity_warning: grouping.identity_warning,
        email_optional: grouping.can_drop_email,
        groups: grouping.groups,
        technical: technical_items(&page.scopes),
        session_token: None,
        accept_action: ACCEPT_ACTION.to_string(),
        reject_action: REJECT_ACTION.to_string(),
        back_href: authorize_href(&page.client_id, &page.request_uri, None),
    })
}

async fn device_session(
    shell: &PageShell,
    shared: &SharedOAuthProvider,
    jar: &CookieJar<'_>,
    info: &OAuthRequestInfo,
    now: u64,
) -> Result<DeviceSession, HtmlPage> {
    ensure_device_session(
        &shared.provider,
        jar,
        info.user_agent.as_deref(),
        &info.ip_address,
        now,
    )
    .await
    .map_err(|error| oauth_error_page(shell, error))
}

/// Runs the provider's authorize decision, yielding the page data to render
/// or the redirect to answer with.
async fn authorize_or_redirect(
    shell: &PageShell,
    shared: &SharedOAuthProvider,
    client_id: &str,
    request_uri: &str,
    session: &DeviceSession,
    now: u64,
) -> Result<AuthorizePageData, Result<Redirect, HtmlPage>> {
    match shared
        .provider
        .authorize(client_id, request_uri, &session.device_id, now)
        .await
    {
        Ok(AuthorizeOutcome::Page(page)) => Ok(*page),
        Ok(AuthorizeOutcome::Redirect(url)) => Err(Ok(Redirect::to(url))),
        Err(error) => Err(Err(oauth_error_page(shell, error))),
    }
}

/// The account a device-authenticated form names, when its session may act
/// for it right now.
async fn linked_account(
    shell: &PageShell,
    shared: &SharedOAuthProvider,
    session: &DeviceSession,
    did: &str,
    now: u64,
) -> Result<(AccountInfo, bool), HtmlPage> {
    match shared
        .provider
        .store()
        .get_device_account_for_session(&session.device_id, &session.session_id, did)
        .await
    {
        Ok(Some(linked)) => {
            let login_required = shared.provider.check_login_required(&linked, now);
            Ok((linked.account, login_required))
        }
        Ok(None) => Err(render_error(
            shell,
            Status::BadRequest,
            "account is not signed in on this device",
        )),
        Err(error) => Err(oauth_error_page(shell, error)),
    }
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::get(
    "/oauth/authorize?<client_id>&<request_uri>&<otp_hint>&<otp_error>&<auth_error>&<view>"
)]
pub async fn oauth_authorize(
    client_id: Option<String>,
    request_uri: Option<String>,
    otp_hint: Option<String>,
    otp_error: Option<bool>,
    auth_error: Option<bool>,
    view: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    sets: &State<SharedPermissionSets>,
    ui: &State<UiState>,
) -> Result<Redirect, HtmlPage> {
    let shell = &ui.shell;
    let (Some(client_id), Some(request_uri)) = (client_id, request_uri) else {
        return Err(render_error(
            shell,
            Status::BadRequest,
            "client_id and request_uri are required",
        ));
    };
    let now = now_secs();
    let session = device_session(shell, shared, jar, &info, now).await?;
    let page =
        match authorize_or_redirect(shell, shared, &client_id, &request_uri, &session, now).await {
            Ok(page) => page,
            Err(answer) => return answer,
        };
    // a second-factor gate in front of this route sends the browser back
    // here with the address hint, after a bad code with an error, and after
    // rejecting the credentials of an account it will not forward without
    // a code
    let otp_error = otp_error.unwrap_or(false);
    let auth_error = auth_error.unwrap_or(false);
    let gated = otp_error || auth_error || otp_hint.as_deref().is_some_and(|h| !h.is_empty());
    if !gated {
        if let Some(selected) = page.selected_did.as_deref() {
            if let Some(info) = page
                .sessions
                .iter()
                .find(|info| info.account.did == selected && !info.login_required)
            {
                let consent = consent_page(ui, sets, &page, &session, &info.account).await?;
                return Err(render_page(Status::Ok, shell, &consent));
            }
        }
    }
    let error = if otp_error {
        Some(CODE_REJECTED.to_string())
    } else if auth_error {
        Some(CREDENTIALS_REJECTED.to_string())
    } else {
        None
    };
    let options = SignInOptions {
        view: (gated || view.as_deref() == Some("sign-in")).then_some(
            if page.login_hint.is_some() {
                SignInView::ForcedIdentifier
            } else {
                SignInView::Form
            },
        ),
        identifier: None,
        error,
        otp_hint: otp_hint.filter(|hint| !hint.is_empty()),
        otp_error,
    };
    Err(render_page(
        Status::Ok,
        shell,
        &sign_in_page(shell, &page, &session, options),
    ))
}

#[derive(FromForm)]
pub struct SignInFormData {
    pub request_uri: String,
    pub client_id: String,
    pub csrf: String,
    pub identifier: String,
    pub password: String,
    /// Consumed by a second-factor gate in front of this route; ignored here.
    pub email_otp: Option<String>,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/authorize/sign-in", data = "<form>")]
pub async fn oauth_authorize_sign_in(
    form: Form<SignInFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    sets: &State<SharedPermissionSets>,
    ui: &State<UiState>,
) -> Result<Redirect, HtmlPage> {
    let shell = &ui.shell;
    let now = now_secs();
    let mut session = device_session(shell, shared, jar, &info, now).await?;
    if form.csrf != session.csrf {
        return Err(render_error(
            shell,
            Status::BadRequest,
            "invalid CSRF token",
        ));
    }
    let signed_in = shared
        .provider
        .sign_in(
            &form.client_id,
            &form.request_uri,
            &session.device_id,
            &form.identifier,
            &form.password,
            true,
            &session.session_id,
            now,
        )
        .await;
    if let Ok(result) = &signed_in {
        if let Some(new_session_id) = &result.new_session_id {
            // the sign-in rotated the device secret: the browser and the
            // forms rendered from here on must carry the new one
            let cookie = device_cookie(&session.device_id, new_session_id);
            session.csrf = csrf_token(cookie.value());
            session.session_id = new_session_id.clone();
            jar.add(cookie);
        }
    }
    let page = match authorize_or_redirect(
        shell,
        shared,
        &form.client_id,
        &form.request_uri,
        &session,
        now,
    )
    .await
    {
        Ok(page) => page,
        Err(answer) => return answer,
    };
    match signed_in {
        Ok(result) => {
            let consent = consent_page(ui, sets, &page, &session, &result.account).await?;
            Err(render_page(Status::Ok, shell, &consent))
        }
        Err(error) => {
            let message = match &error {
                OAuthError::InvalidRequest(description)
                    if description == "invalid identifier or password" =>
                {
                    CREDENTIALS_REJECTED.to_string()
                }
                other => other.error_description().to_string(),
            };
            let options = SignInOptions {
                view: Some(if page.login_hint.is_some() {
                    SignInView::ForcedIdentifier
                } else {
                    SignInView::Form
                }),
                identifier: Some(form.identifier.clone()),
                error: Some(message),
                ..SignInOptions::default()
            };
            Err(render_page(
                Status::Ok,
                shell,
                &sign_in_page(shell, &page, &session, options),
            ))
        }
    }
}

#[derive(FromForm)]
pub struct SelectAccountFormData {
    pub request_uri: String,
    pub client_id: String,
    pub csrf: String,
    pub did: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/authorize/select", data = "<form>")]
pub async fn oauth_authorize_select(
    form: Form<SelectAccountFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    sets: &State<SharedPermissionSets>,
    ui: &State<UiState>,
) -> Result<Redirect, HtmlPage> {
    let shell = &ui.shell;
    let now = now_secs();
    let session = device_session(shell, shared, jar, &info, now).await?;
    if form.csrf != session.csrf {
        return Err(render_error(
            shell,
            Status::BadRequest,
            "invalid CSRF token",
        ));
    }
    let (account, login_required) = linked_account(shell, shared, &session, &form.did, now).await?;
    let page = match authorize_or_redirect(
        shell,
        shared,
        &form.client_id,
        &form.request_uri,
        &session,
        now,
    )
    .await
    {
        Ok(page) => page,
        Err(answer) => return answer,
    };
    let login_required = login_required
        || page
            .sessions
            .iter()
            .any(|info| info.account.did == account.did && info.login_required);
    if login_required {
        // the session is too old to act on its own: confirm the password
        let options = SignInOptions {
            view: Some(SignInView::ConfirmSelected),
            identifier: Some(
                account
                    .handle
                    .clone()
                    .unwrap_or_else(|| account.did.clone()),
            ),
            ..SignInOptions::default()
        };
        return Err(render_page(
            Status::Ok,
            shell,
            &sign_in_page(shell, &page, &session, options),
        ));
    }
    let consent = consent_page(ui, sets, &page, &session, &account).await?;
    Err(render_page(Status::Ok, shell, &consent))
}

#[derive(FromForm)]
pub struct ConsentFormData {
    pub request_uri: String,
    pub client_id: String,
    pub csrf: String,
    pub did: Option<String>,
    /// The scope the page showed, so the email grant can be declined.
    pub scope: Option<String>,
    /// Present when the page offered to decline the email grant.
    pub email_optional: Option<String>,
    /// Present when the email grant was left checked.
    pub allow_email: Option<String>,
}

impl ConsentFormData {
    /// The scope to grant: everything requested, minus the email grant when
    /// the page offered the choice and the box was unchecked.
    fn granted_scope(&self) -> Option<String> {
        let scope = self.scope.as_deref()?;
        if self.email_optional.is_none() || self.allow_email.is_some() {
            return Some(scope.to_string());
        }
        Some(
            scope
                .split_ascii_whitespace()
                .filter(|s| *s != "account:email" && !s.starts_with("account:email?"))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/authorize/accept", data = "<form>")]
pub async fn oauth_authorize_accept(
    form: Form<ConsentFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, HtmlPage> {
    let shell = &ui.shell;
    let now = now_secs();
    let session = device_session(shell, shared, jar, &info, now).await?;
    if form.csrf != session.csrf {
        return Err(render_error(
            shell,
            Status::BadRequest,
            "invalid CSRF token",
        ));
    }
    let Some(did) = form.did.clone() else {
        return Err(render_error(shell, Status::BadRequest, "did is required"));
    };
    let granted = form.granted_scope();
    let redirect = shared
        .provider
        .accept(
            &form.client_id,
            &form.request_uri,
            &session.device_id,
            &did,
            AccountProof::Device {
                session_id: &session.session_id,
            },
            granted.as_deref(),
            now,
        )
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    record_oauth_authorization_granted(shared.provider.is_trusted_client(&form.client_id));
    Ok(Redirect::to(redirect))
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/authorize/reject", data = "<form>")]
pub async fn oauth_authorize_reject(
    form: Form<ConsentFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, HtmlPage> {
    let shell = &ui.shell;
    let now = now_secs();
    let session = device_session(shell, shared, jar, &info, now).await?;
    if form.csrf != session.csrf {
        return Err(render_error(
            shell,
            Status::BadRequest,
            "invalid CSRF token",
        ));
    }
    shared
        .provider
        .reject(&form.client_id, &form.request_uri, &session.device_id, now)
        .await
        .map(Redirect::to)
        .map_err(|error| oauth_error_page(shell, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorize_hrefs_encode_their_parameters() {
        assert_eq!(
            authorize_href("http://localhost?x=1", "urn:ietf:params:oauth:request_uri:r", None),
            "/oauth/authorize?client_id=http%3A%2F%2Flocalhost%3Fx%3D1&request_uri=urn%3Aietf%3Aparams%3Aoauth%3Arequest_uri%3Ar"
        );
        assert!(authorize_href("c", "r", Some("sign-in")).ends_with("&view=sign-in"));
    }

    #[test]
    fn granted_scope_drops_email_only_when_offered_and_unchecked() {
        let mut form = ConsentFormData {
            request_uri: "r".into(),
            client_id: "c".into(),
            csrf: "t".into(),
            did: None,
            scope: None,
            email_optional: None,
            allow_email: None,
        };
        assert_eq!(form.granted_scope(), None);
        form.scope = Some("atproto account:email?action=manage repo:a.b.c".into());
        assert_eq!(
            form.granted_scope().as_deref(),
            Some("atproto account:email?action=manage repo:a.b.c")
        );
        form.email_optional = Some("1".into());
        assert_eq!(form.granted_scope().as_deref(), Some("atproto repo:a.b.c"));
        form.allow_email = Some("on".into());
        assert_eq!(
            form.granted_scope().as_deref(),
            Some("atproto account:email?action=manage repo:a.b.c")
        );
        form.scope = Some("atproto account:email".into());
        form.allow_email = None;
        assert_eq!(form.granted_scope().as_deref(), Some("atproto"));
    }

    #[test]
    fn is_new_oauth_session_only_true_for_authorization_code() {
        assert!(is_new_oauth_session(
            rsky_oauth::types::GRANT_AUTHORIZATION_CODE
        ));
        assert!(!is_new_oauth_session(
            rsky_oauth::types::GRANT_REFRESH_TOKEN
        ));
        assert!(!is_new_oauth_session("something_else"));
    }
}
