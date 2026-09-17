use super::body::OAuthBody;
use super::{
    cookie_test_cookie, csrf_token, device_cookie, ensure_device_session, now_secs, DeviceSession,
    SharedOAuthProvider, COOKIE_TEST,
};
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::server::activate_account::activate_account_for;
use crate::metrics::{record_login_success, record_oauth_authorization_granted};
use crate::oauth_scope::INCLUDE_PREFIX;
use crate::permission_set::SharedPermissionSets;
use crate::ui::client::client_view;
use crate::ui::pages::oauth::{
    ConsentPage, CookieErrorPage, ErrorPage, ReactivatePage, SignInPage, SignInView, WelcomePage,
};
use crate::ui::pages::AccountCardView;
use crate::ui::respond::{render_page, UiHtml};
use crate::ui::scopes::{permission_groups, IncludeSetView};
use crate::ui::shell::PageShell;
use crate::ui::technical::technical_items;
use crate::ui::UiState;
use crate::SharedSequencer;
use rocket::form::Form;
use rocket::http::{ContentType, CookieJar, Header, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::{Redirect, Responder, Response};
use rocket::serde::json::Json;
use rocket::FromForm;
use rocket::State;
use rsky_oauth::client::ParRequest;
use rsky_oauth::dpop::DpopRequest;
use rsky_oauth::store::AccountInfo;
use rsky_oauth::types::GRANT_AUTHORIZATION_CODE;
use rsky_oauth::{
    generate_session_id, AccountProof, AuthorizeOutcome, AuthorizePageData, ClientCredentials,
    OAuthError, TokenRequest,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Cursor;
use url::Url;

/// Request material needed to validate DPoP proofs.
pub struct OAuthRequestInfo {
    pub method: String,
    pub uri: String,
    pub dpop_headers: Vec<String>,
    pub user_agent: Option<String>,
    pub ip_address: String,
    /// The request carries fetch metadata or an Origin naming another site
    pub cross_site: bool,
}

/// Whether the request came from another site, judged by the browser's
/// own fetch metadata and Origin header. Neither header being present is
/// not held against the request, so a proxy posting a gated form still
/// passes.
fn is_cross_site(sec_fetch_site: Option<&str>, origin: Option<&str>, public_url: &str) -> bool {
    if let Some(site) = sec_fetch_site {
        if site.eq_ignore_ascii_case("cross-site") {
            return true;
        }
    }
    match origin {
        None => false,
        Some(origin) => match (Url::parse(origin), Url::parse(public_url)) {
            (Ok(origin), Ok(own)) => origin.origin() != own.origin(),
            _ => true,
        },
    }
}

fn is_ios(user_agent: Option<&str>) -> bool {
    user_agent.is_some_and(|ua| ["iPhone", "iPad", "iPod"].iter().any(|d| ua.contains(d)))
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
            cross_site: is_cross_site(
                req.headers().get_one("sec-fetch-site"),
                req.headers().get_one("origin"),
                &cfg.service.public_url,
            ),
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
const REACTIVATE_ACTION: &str = "/oauth/authorize/reactivate";
const AUTHORIZE_PATH: &str = "/oauth/authorize";
const CREDENTIALS_REJECTED: &str = "Invalid identifier or password";
const CODE_REJECTED: &str = "The sign-in code was not accepted";
const PERMISSION_SETS_UNAVAILABLE: &str = "Unable to retrieve permission sets";
const SESSION_CHANGED: &str = "Your session changed in another tab. Please try again.";
const CROSS_SITE_POST: &str = "This request came from another site and was not accepted.";
const COOKIES_UNSUPPORTED: &str =
    "Your browser does not accept cookies, so sign-in cannot continue.";

fn authorize_href(client_id: &str, request_uri: &str, view: Option<&str>) -> String {
    let mut pairs = vec![("client_id", client_id), ("request_uri", request_uri)];
    if let Some(view) = view {
        pairs.push(("view", view));
    }
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish();
    format!("{AUTHORIZE_PATH}?{query}")
}

/// What the sign-in page is asked to show on top of the request itself.
#[derive(Default)]
struct SignInOptions {
    view: Option<SignInView>,
    identifier: Option<String>,
    error: Option<String>,
    otp_hint: Option<String>,
    otp_error: bool,
    remember_checked: bool,
}

fn sign_in_page(
    ui: &UiState,
    page: &AuthorizePageData,
    session: &DeviceSession,
    options: SignInOptions,
) -> SignInPage {
    let shell = &ui.shell;
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
    // the picker goes back to the welcome view when sign-up is offered;
    // the form goes back to the picker, or to the welcome view
    let back_href = match view {
        SignInView::Picker => ui
            .signup_url
            .is_some()
            .then(|| authorize_href(&page.client_id, &page.request_uri, Some("welcome"))),
        _ if !page.sessions.is_empty() => {
            Some(authorize_href(&page.client_id, &page.request_uri, None))
        }
        _ => ui
            .signup_url
            .is_some()
            .then(|| authorize_href(&page.client_id, &page.request_uri, Some("welcome"))),
    }
    .unwrap_or_default();
    SignInPage {
        shell: (**shell).clone(),
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
        show_remember: true,
        remember_checked: options.remember_checked,
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
        signup_href: ui.signup_url.clone(),
        forgot_href: None,
        back_href,
        back_label: "Back".to_string(),
    }
}

fn welcome_page(ui: &UiState, page: &AuthorizePageData, session: &DeviceSession) -> WelcomePage {
    WelcomePage {
        shell: (*ui.shell).clone(),
        csrf: session.csrf.clone(),
        client_id: page.client_id.clone(),
        request_uri: page.request_uri.clone(),
        signup_href: ui.signup_url.clone().unwrap_or_default(),
        sign_in_href: authorize_href(&page.client_id, &page.request_uri, Some("sign-in")),
        cancel_action: REJECT_ACTION.to_string(),
    }
}

fn reactivate_page(
    ui: &UiState,
    page: &AuthorizePageData,
    session: &DeviceSession,
    account: &AccountInfo,
    session_token: Option<String>,
    error: Option<String>,
) -> ReactivatePage {
    ReactivatePage {
        shell: (*ui.shell).clone(),
        csrf: session.csrf.clone(),
        client_id: page.client_id.clone(),
        request_uri: page.request_uri.clone(),
        account: AccountCardView::from_account(account, false),
        session_token,
        error,
        reactivate_action: REACTIVATE_ACTION.to_string(),
        cancel_action: REJECT_ACTION.to_string(),
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
    session_token: Option<String>,
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
        session_token,
        accept_action: ACCEPT_ACTION.to_string(),
        reject_action: REJECT_ACTION.to_string(),
        back_href: authorize_href(&page.client_id, &page.request_uri, None),
    })
}

/// The screen after an account is established for the request: the
/// reactivation offer for a deactivated account, otherwise consent.
async fn account_page(
    ui: &UiState,
    sets: &SharedPermissionSets,
    page: &AuthorizePageData,
    session: &DeviceSession,
    account: &AccountInfo,
    session_token: Option<String>,
) -> Result<HtmlPage, HtmlPage> {
    if account.deactivated {
        return Ok(render_page(
            Status::Ok,
            &ui.shell,
            &reactivate_page(ui, page, session, account, session_token, None),
        ));
    }
    let consent = consent_page(ui, sets, page, session, account, session_token).await?;
    Ok(render_page(Status::Ok, &ui.shell, &consent))
}

async fn device_session(
    shell: &PageShell,
    shared: &SharedOAuthProvider,
    jar: &CookieJar<'_>,
    info: &OAuthRequestInfo,
    now: u64,
) -> Result<DeviceSession, HtmlPage> {
    ensure_device_session(
        shared,
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

/// The proof a form carries for its account: the ephemeral token of a
/// sign-in that was not remembered, else the device session.
fn account_proof<'a>(
    session: &'a DeviceSession,
    session_token: Option<&'a str>,
) -> AccountProof<'a> {
    match session_token {
        Some(token) => AccountProof::Ephemeral(token),
        None => AccountProof::Device {
            session_id: &session.session_id,
        },
    }
}

/// Refuses a form posted from another site or with a csrf token from
/// before the session changed, with the page a destructive form gets.
fn check_form_origin(
    shell: &PageShell,
    info: &OAuthRequestInfo,
    session: &DeviceSession,
    csrf: &str,
    back_href: String,
) -> Result<(), HtmlPage> {
    if info.cross_site {
        return Err(render_page(
            Status::Forbidden,
            shell,
            &ErrorPage::with_back(shell.clone(), CROSS_SITE_POST, back_href),
        ));
    }
    if csrf != session.csrf {
        return Err(render_page(
            Status::BadRequest,
            shell,
            &ErrorPage::with_back(shell.clone(), SESSION_CHANGED, back_href),
        ));
    }
    Ok(())
}

/// Rotates the device secret after a privilege transition and hands the
/// browser the new cookie. A device whose secret changed underneath the
/// request keeps whatever secret it has now.
async fn rotate_session(
    shared: &SharedOAuthProvider,
    jar: &CookieJar<'_>,
    session: &mut DeviceSession,
) -> Result<(), OAuthError> {
    let new_session_id = generate_session_id();
    match shared
        .provider
        .store()
        .rotate_device_session(&session.device_id, &session.session_id, &new_session_id)
        .await
    {
        Ok(()) => {
            adopt_session(shared, jar, session, new_session_id);
            Ok(())
        }
        Err(OAuthError::InvalidRequest(reason)) if reason == "device session changed" => Ok(()),
        Err(error) => Err(error),
    }
}

/// The browser and the forms rendered from here on carry the new secret.
fn adopt_session(
    shared: &SharedOAuthProvider,
    jar: &CookieJar<'_>,
    session: &mut DeviceSession,
    new_session_id: String,
) {
    let cookie = device_cookie(&session.device_id, &new_session_id, shared.secure_cookies);
    session.csrf = csrf_token(cookie.value());
    session.session_id = new_session_id;
    jar.add(cookie);
}

#[derive(FromForm)]
pub struct AuthorizeQuery {
    pub client_id: Option<String>,
    pub request_uri: Option<String>,
    /// A second-factor gate in front of this route sends the browser back
    /// here with the address hint, after a bad code with an error, and
    /// after rejecting the credentials of an account it will not forward
    /// without a code; `remember` keeps the checkbox through those hops.
    pub otp_hint: Option<String>,
    pub otp_error: Option<bool>,
    pub auth_error: Option<bool>,
    pub remember: Option<String>,
    pub view: Option<String>,
    /// Set on the cookie probe's return trip
    #[field(name = "redirect-test")]
    pub redirect_test: Option<String>,
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::get("/oauth/authorize?<query..>")]
pub async fn oauth_authorize(
    query: AuthorizeQuery,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    sets: &State<SharedPermissionSets>,
    ui: &State<UiState>,
) -> Result<Redirect, HtmlPage> {
    let shell = &ui.shell;
    let (Some(client_id), Some(request_uri)) = (query.client_id, query.request_uri) else {
        return Err(render_error(
            shell,
            Status::BadRequest,
            "client_id and request_uri are required",
        ));
    };
    let now = now_secs();
    // browsers known to drop cookies in some embedded contexts are asked
    // to come back once with a probe cookie before anything is bound to
    // this device
    if is_ios(info.user_agent.as_deref()) && jar.get(COOKIE_TEST).is_none() {
        if query.redirect_test.is_none() {
            jar.add(cookie_test_cookie(shared.secure_cookies));
            let page = CookieErrorPage {
                shell: (**shell).clone(),
                cookie_message: CookieErrorPage::message(&shell.hostname),
                continue_action: AUTHORIZE_PATH.to_string(),
                continue_params: vec![
                    ("client_id".to_string(), client_id),
                    ("request_uri".to_string(), request_uri),
                    ("redirect-test".to_string(), "1".to_string()),
                ],
            };
            return Err(render_page(Status::Ok, shell, &page));
        }
        let session = device_session(shell, shared, jar, &info, now).await?;
        return match shared
            .provider
            .abandon_request(
                &client_id,
                &request_uri,
                &session.device_id,
                "invalid_request",
                "ERR_COOKIES_UNSUPPORTED",
                now,
            )
            .await
        {
            Ok(url) => Ok(Redirect::to(url)),
            Err(_) => Err(render_error(shell, Status::BadRequest, COOKIES_UNSUPPORTED)),
        };
    }
    let session = device_session(shell, shared, jar, &info, now).await?;
    let page =
        match authorize_or_redirect(shell, shared, &client_id, &request_uri, &session, now).await {
            Ok(page) => page,
            Err(answer) => return answer,
        };
    let otp_error = query.otp_error.unwrap_or(false);
    let auth_error = query.auth_error.unwrap_or(false);
    let otp_hint = query.otp_hint.filter(|hint| !hint.is_empty());
    let gated = otp_error || auth_error || otp_hint.is_some();
    let view = query.view.as_deref();
    if !gated {
        if let Some(selected) = page.selected_did.as_deref() {
            if let Some(info) = page
                .sessions
                .iter()
                .find(|info| info.account.did == selected && !info.login_required)
            {
                return Err(account_page(ui, sets, &page, &session, &info.account, None)
                    .await
                    .unwrap_or_else(|error| error));
            }
        }
        let welcome = ui.signup_url.is_some()
            && page.login_hint.is_none()
            && (view == Some("welcome") || (view.is_none() && page.sessions.is_empty()));
        if welcome {
            return Err(render_page(
                Status::Ok,
                shell,
                &welcome_page(ui, &page, &session),
            ));
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
        view: (gated || view == Some("sign-in")).then_some(if page.login_hint.is_some() {
            SignInView::ForcedIdentifier
        } else {
            SignInView::Form
        }),
        identifier: None,
        error,
        otp_hint,
        otp_error,
        remember_checked: query.remember.as_deref() == Some("1"),
    };
    Err(render_page(
        Status::Ok,
        shell,
        &sign_in_page(ui, &page, &session, options),
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
    /// `on` when the account is to stay signed in on this device.
    pub remember: Option<String>,
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
    if info.cross_site {
        return Err(render_page(
            Status::Forbidden,
            shell,
            &ErrorPage::with_back(
                (**shell).clone(),
                CROSS_SITE_POST,
                authorize_href(&form.client_id, &form.request_uri, None),
            ),
        ));
    }
    let remember = form.remember.as_deref() == Some("on");
    let form_view = |page: &AuthorizePageData| {
        if page.login_hint.is_some() {
            SignInView::ForcedIdentifier
        } else {
            SignInView::Form
        }
    };
    if form.csrf != session.csrf {
        // the secret changed in another tab: the same form again, with the
        // token that matches the cookie the browser now holds
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
        let options = SignInOptions {
            view: Some(form_view(&page)),
            identifier: Some(form.identifier.clone()),
            error: Some(SESSION_CHANGED.to_string()),
            remember_checked: remember,
            ..SignInOptions::default()
        };
        return Err(render_page(
            Status::Ok,
            shell,
            &sign_in_page(ui, &page, &session, options),
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
            remember,
            &session.session_id,
            now,
        )
        .await;
    if let Ok(result) = &signed_in {
        if let Some(new_session_id) = &result.new_session_id {
            adopt_session(shared, jar, &mut session, new_session_id.clone());
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
        Ok(result) => Err(account_page(
            ui,
            sets,
            &page,
            &session,
            &result.account,
            result.ephemeral_token,
        )
        .await
        .unwrap_or_else(|error| error)),
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
                view: Some(form_view(&page)),
                identifier: Some(form.identifier.clone()),
                error: Some(message),
                remember_checked: remember,
                ..SignInOptions::default()
            };
            Err(render_page(
                Status::Ok,
                shell,
                &sign_in_page(ui, &page, &session, options),
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
    if info.cross_site {
        return Err(render_page(
            Status::Forbidden,
            shell,
            &ErrorPage::with_back(
                (**shell).clone(),
                CROSS_SITE_POST,
                authorize_href(&form.client_id, &form.request_uri, None),
            ),
        ));
    }
    if form.csrf != session.csrf {
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
        let options = SignInOptions {
            view: (page.sessions.is_empty()).then_some(SignInView::Form),
            error: Some(SESSION_CHANGED.to_string()),
            ..SignInOptions::default()
        };
        return Err(render_page(
            Status::Ok,
            shell,
            &sign_in_page(ui, &page, &session, options),
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
            remember_checked: true,
            ..SignInOptions::default()
        };
        return Err(render_page(
            Status::Ok,
            shell,
            &sign_in_page(ui, &page, &session, options),
        ));
    }
    Err(account_page(ui, sets, &page, &session, &account, None)
        .await
        .unwrap_or_else(|error| error))
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
    /// The proof of a sign-in that was not remembered on this device.
    pub session_token: Option<String>,
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
    check_form_origin(
        shell,
        &info,
        &session,
        &form.csrf,
        authorize_href(&form.client_id, &form.request_uri, None),
    )?;
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
            account_proof(&session, form.session_token.as_deref()),
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
    let mut session = device_session(shell, shared, jar, &info, now).await?;
    check_form_origin(
        shell,
        &info,
        &session,
        &form.csrf,
        authorize_href(&form.client_id, &form.request_uri, None),
    )?;
    let redirect = shared
        .provider
        .reject(&form.client_id, &form.request_uri, &session.device_id, now)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    rotate_session(shared, jar, &mut session)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    Ok(Redirect::to(redirect))
}

#[derive(FromForm)]
pub struct ReactivateFormData {
    pub request_uri: String,
    pub client_id: String,
    pub csrf: String,
    pub did: String,
    pub session_token: Option<String>,
}

/// Reactivates the account the flow just authenticated, then continues to
/// consent. The proof is the same one `accept` takes.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/authorize/reactivate", data = "<form>")]
pub async fn oauth_authorize_reactivate(
    form: Form<ReactivateFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    sets: &State<SharedPermissionSets>,
    ui: &State<UiState>,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    account_manager: &State<AccountManager>,
) -> Result<Redirect, HtmlPage> {
    let shell = &ui.shell;
    let now = now_secs();
    let session = device_session(shell, shared, jar, &info, now).await?;
    check_form_origin(
        shell,
        &info,
        &session,
        &form.csrf,
        authorize_href(&form.client_id, &form.request_uri, None),
    )?;
    let account = match form.session_token.as_deref() {
        Some(token) => {
            shared
                .provider
                .verify_ephemeral_token(
                    token,
                    &form.did,
                    &session.device_id,
                    &form.request_uri,
                    now,
                )
                .map_err(|error| oauth_error_page(shell, error))?;
            match shared.provider.store().get_account(&form.did).await {
                Ok(Some(account)) => account,
                Ok(None) => return Err(render_error(shell, Status::BadRequest, "unknown account")),
                Err(error) => return Err(oauth_error_page(shell, error)),
            }
        }
        None => {
            let (account, login_required) =
                linked_account(shell, shared, &session, &form.did, now).await?;
            if login_required {
                return Err(render_page(
                    Status::BadRequest,
                    shell,
                    &ErrorPage::with_back(
                        (**shell).clone(),
                        "Please sign in again to reactivate this account.",
                        authorize_href(&form.client_id, &form.request_uri, None),
                    ),
                ));
            }
            account
        }
    };
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
    if account.deactivated {
        if let Err(error) = activate_account_for(
            account.did.clone(),
            sequencer,
            blobstore_factory,
            actor_store,
            account_manager,
        )
        .await
        {
            tracing::warn!(%error, did = %account.did, "reactivation from the authorization flow failed");
            return Err(render_page(
                Status::Ok,
                shell,
                &reactivate_page(
                    ui,
                    &page,
                    &session,
                    &account,
                    form.session_token.clone(),
                    Some("Something went wrong".to_string()),
                ),
            ));
        }
    }
    let account = AccountInfo {
        deactivated: false,
        ..account
    };
    // the request may no longer need consent now that the account is active
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
    Err(account_page(
        ui,
        sets,
        &page,
        &session,
        &account,
        form.session_token.clone(),
    )
    .await
    .unwrap_or_else(|error| error))
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
            session_token: None,
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

    #[tokio::test]
    async fn include_sets_resolve_once_per_set_and_fail_closed() {
        let sets = SharedPermissionSets::default();
        sets.resolver
            .prime("app.example.set", vec!["repo:app.example.record".into()])
            .await;
        let views = include_sets(
            &sets,
            &[
                "atproto".into(),
                "include:app.example.set".into(),
                "include:app.example.set".into(),
            ],
        )
        .await
        .unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views["app.example.set"].scopes, ["repo:app.example.record"]);
        assert!(views["app.example.set"].title.is_none());
        let error = include_sets(&sets, &["include:invalid.example.nothing".into()])
            .await
            .unwrap_err();
        assert_eq!(error, PERMISSION_SETS_UNAVAILABLE);
    }

    #[test]
    fn cross_site_posts_are_told_apart_by_fetch_metadata_and_origin() {
        let own = "https://pds.test";
        assert!(!is_cross_site(None, None, own));
        assert!(!is_cross_site(Some("same-origin"), None, own));
        assert!(!is_cross_site(Some("none"), Some("https://pds.test"), own));
        assert!(is_cross_site(Some("cross-site"), None, own));
        assert!(is_cross_site(
            Some("Cross-Site"),
            Some("https://pds.test"),
            own
        ));
        assert!(!is_cross_site(None, Some("https://pds.test"), own));
        assert!(!is_cross_site(None, Some("HTTPS://PDS.TEST/"), own));
        assert!(is_cross_site(None, Some("https://evil.test"), own));
        assert!(is_cross_site(None, Some("null"), own));
        assert!(is_cross_site(None, Some("https://pds.test"), "not a url"));
        assert!(is_ios(Some(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)"
        )));
        assert!(is_ios(Some(
            "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X)"
        )));
        assert!(!is_ios(Some(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)"
        )));
        assert!(!is_ios(None));
    }

    #[test]
    fn account_proofs_prefer_the_ephemeral_token() {
        let session = DeviceSession {
            device_id: "dev-1".into(),
            session_id: "ses-1".into(),
            csrf: "c".into(),
        };
        assert_eq!(
            account_proof(&session, Some("tok")),
            AccountProof::Ephemeral("tok")
        );
        assert_eq!(
            account_proof(&session, None),
            AccountProof::Device {
                session_id: "ses-1"
            }
        );
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
