use super::templates::{
    client_display, scope_items, ConsentPage, ErrorPage, SessionOption, SignInPage,
};
use super::{ensure_device_session, now_secs, DeviceSession, SharedOAuthProvider};
use askama::Template;
use rocket::form::Form;
use rocket::http::{ContentType, CookieJar, Header, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::content::RawHtml;
use rocket::response::{Redirect, Responder, Response};
use rocket::serde::json::Json;
use rocket::FromForm;
use rocket::State;
use rsky_common::env::env_str;
use rsky_oauth::client::ParRequest;
use rsky_oauth::dpop::DpopRequest;
use rsky_oauth::store::AccountInfo;
use rsky_oauth::{AuthorizePageData, ClientCredentials, OAuthError, TokenRequest};
use serde_json::Value;
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

type HtmlPage = (Status, RawHtml<String>);

fn render_error(status: Status, message: impl Into<String>) -> HtmlPage {
    let page = ErrorPage {
        message: message.into(),
    };
    (
        status,
        RawHtml(page.render().expect("error template rendering cannot fail")),
    )
}

fn oauth_error_page(error: OAuthError) -> HtmlPage {
    render_error(Status::new(error.status()), error.error_description())
}

#[derive(FromForm)]
pub struct ParFormData {
    pub client_id: Option<String>,
    pub response_type: Option<String>,
    pub redirect_uri: Option<String>,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub login_hint: Option<String>,
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
            prompt: self.prompt.clone(),
        }
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/par", data = "<form>")]
pub async fn oauth_par(
    form: Form<ParFormData>,
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

#[derive(FromForm)]
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
    form: Form<TokenFormData>,
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
    match provider
        .token(
            &credentials,
            &request,
            &info.dpop_request(&headers, None),
            now,
        )
        .await
    {
        Ok(response) => OAuthApiResponse::ok(
            Status::Ok,
            serde_json::to_value(response).expect("token response serialization cannot fail"),
            nonce,
        ),
        Err(error) => OAuthApiResponse::error(error, nonce),
    }
}

#[derive(FromForm)]
pub struct RevokeFormData {
    pub token: Option<String>,
    pub client_id: Option<String>,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/revoke", data = "<form>")]
pub async fn oauth_revoke(
    form: Form<RevokeFormData>,
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

fn account_label(account: &AccountInfo) -> String {
    account
        .handle
        .clone()
        .unwrap_or_else(|| account.did.clone())
}

fn sign_in_page(
    page: &AuthorizePageData,
    session: &DeviceSession,
    error: Option<String>,
) -> SignInPage {
    SignInPage {
        client_display: client_display(page),
        client_id: page.client_id.clone(),
        request_uri: page.request_uri.clone(),
        csrf: session.csrf.clone(),
        login_hint: page.login_hint.clone().unwrap_or_default(),
        error,
        signup_url: env_str("PDS_OAUTH_SIGNUP_URL"),
        sessions: page
            .sessions
            .iter()
            .map(|account| SessionOption {
                did: account.did.clone(),
                label: account_label(account),
            })
            .collect(),
    }
}

fn consent_page(
    page: &AuthorizePageData,
    session: &DeviceSession,
    account: &AccountInfo,
) -> ConsentPage {
    ConsentPage {
        client_display: client_display(page),
        client_id: page.client_id.clone(),
        client_trusted: page.client_trusted,
        request_uri: page.request_uri.clone(),
        csrf: session.csrf.clone(),
        did: account.did.clone(),
        account_label: account_label(account),
        scopes: scope_items(&page.scopes),
    }
}

fn render<T: Template>(status: Status, template: &T) -> HtmlPage {
    match template.render() {
        Ok(html) => (status, RawHtml(html)),
        Err(error) => render_error(Status::InternalServerError, error.to_string()),
    }
}

async fn device_session(
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
    .map_err(oauth_error_page)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/oauth/authorize?<client_id>&<request_uri>")]
pub async fn oauth_authorize(
    client_id: Option<String>,
    request_uri: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
) -> HtmlPage {
    let (Some(client_id), Some(request_uri)) = (client_id, request_uri) else {
        return render_error(Status::BadRequest, "client_id and request_uri are required");
    };
    let now = now_secs();
    let session = match device_session(shared, jar, &info, now).await {
        Ok(session) => session,
        Err(page) => return page,
    };
    match shared
        .provider
        .authorize(&client_id, &request_uri, &session.device_id, now)
        .await
    {
        Ok(page) => render(Status::Ok, &sign_in_page(&page, &session, None)),
        Err(error) => oauth_error_page(error),
    }
}

#[derive(FromForm)]
pub struct SignInFormData {
    pub request_uri: String,
    pub client_id: String,
    pub csrf: String,
    pub identifier: String,
    pub password: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/authorize/sign-in", data = "<form>")]
pub async fn oauth_authorize_sign_in(
    form: Form<SignInFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
) -> HtmlPage {
    let now = now_secs();
    let session = match device_session(shared, jar, &info, now).await {
        Ok(session) => session,
        Err(page) => return page,
    };
    if form.csrf != session.csrf {
        return render_error(Status::BadRequest, "invalid CSRF token");
    }
    let signed_in = shared
        .provider
        .sign_in(
            &form.client_id,
            &form.request_uri,
            &session.device_id,
            &form.identifier,
            &form.password,
            now,
        )
        .await;
    let page = match shared
        .provider
        .authorize(&form.client_id, &form.request_uri, &session.device_id, now)
        .await
    {
        Ok(page) => page,
        Err(error) => return oauth_error_page(error),
    };
    match signed_in {
        Ok(account) => render(Status::Ok, &consent_page(&page, &session, &account)),
        Err(error) => render(
            Status::Ok,
            &sign_in_page(&page, &session, Some(error.error_description().to_string())),
        ),
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
) -> HtmlPage {
    let now = now_secs();
    let session = match device_session(shared, jar, &info, now).await {
        Ok(session) => session,
        Err(page) => return page,
    };
    if form.csrf != session.csrf {
        return render_error(Status::BadRequest, "invalid CSRF token");
    }
    let account = match shared
        .provider
        .store()
        .get_device_account(&session.device_id, &form.did)
        .await
    {
        Ok(Some(account)) => account,
        Ok(None) => {
            return render_error(
                Status::BadRequest,
                "account is not signed in on this device",
            )
        }
        Err(error) => return oauth_error_page(error),
    };
    match shared
        .provider
        .authorize(&form.client_id, &form.request_uri, &session.device_id, now)
        .await
    {
        Ok(page) => render(Status::Ok, &consent_page(&page, &session, &account)),
        Err(error) => oauth_error_page(error),
    }
}

#[derive(FromForm)]
pub struct ConsentFormData {
    pub request_uri: String,
    pub client_id: String,
    pub csrf: String,
    pub did: Option<String>,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/authorize/accept", data = "<form>")]
pub async fn oauth_authorize_accept(
    form: Form<ConsentFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
) -> Result<Redirect, HtmlPage> {
    let now = now_secs();
    let session = device_session(shared, jar, &info, now).await?;
    if form.csrf != session.csrf {
        return Err(render_error(Status::BadRequest, "invalid CSRF token"));
    }
    let Some(did) = form.did.clone() else {
        return Err(render_error(Status::BadRequest, "did is required"));
    };
    shared
        .provider
        .accept(
            &form.client_id,
            &form.request_uri,
            &session.device_id,
            &did,
            now,
        )
        .await
        .map(Redirect::to)
        .map_err(oauth_error_page)
}

#[tracing::instrument(skip_all)]
#[rocket::post("/oauth/authorize/reject", data = "<form>")]
pub async fn oauth_authorize_reject(
    form: Form<ConsentFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
) -> Result<Redirect, HtmlPage> {
    let now = now_secs();
    let session = device_session(shared, jar, &info, now).await?;
    if form.csrf != session.csrf {
        return Err(render_error(Status::BadRequest, "invalid CSRF token"));
    }
    shared
        .provider
        .reject(&form.client_id, &form.request_uri, &session.device_id, now)
        .await
        .map(Redirect::to)
        .map_err(oauth_error_page)
}
