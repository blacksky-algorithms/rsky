use crate::oauth::routes::{
    adopt_session, check_form_origin, device_session, oauth_error_page, render_error,
    rotate_session, OAuthRequestInfo, CODE_REJECTED, CREDENTIALS_REJECTED, CROSS_SITE_POST,
    SESSION_CHANGED,
};
use crate::oauth::{now_secs, DeviceSession, SharedOAuthProvider};
use crate::ui::client::{client_app_name, client_identifier};
use crate::ui::format::{browser_name, calendar_date, date_ago};
use crate::ui::pages::account::{
    AboutPage, AccountNav, AppDetailsPage, AppRow, AppsPage, DeviceRow, DevicesPage, HomePage,
    Section,
};
use crate::ui::pages::oauth::{ErrorPage, SignInPage, SignInView, WelcomePage};
use crate::ui::pages::AccountCardView;
use crate::ui::respond::{render_page, UiHtml};
use crate::ui::scopes::permission_groups;
use crate::ui::UiState;
use rocket::form::Form;
use rocket::http::{CookieJar, Status};
use rocket::response::Redirect;
use rocket::{FromForm, Route, State};
use rsky_oauth::generate_session_id;
use rsky_oauth::store::{AccountInfo, DeviceAccount};
use rsky_oauth::OAuthError;
use std::collections::BTreeMap;

const ACCOUNT_PATH: &str = "/account";
pub(super) const SIGN_IN_PATH: &str = "/account/sign-in";
const SELECT_PATH: &str = "/account/select";
const CANNOT_REMOVE_CURRENT_DEVICE: &str = "Cannot remove current device";

/// Every route of the account manager, mounted when the pages are enabled.
pub fn routes() -> Vec<Route> {
    let mut routes = rocket::routes![
        account_index,
        account_sign_in_page,
        account_sign_in,
        account_select,
        account_sign_out_all,
        account_home,
        account_sign_out,
        account_devices,
        account_device_sign_out,
        account_apps,
        account_app_details,
        account_app_revoke,
        account_about,
        account_not_found,
    ];
    routes.extend(super::manage::routes());
    routes.extend(super::reset::routes());
    routes.extend(super::lifecycle::routes());
    routes.extend(super::signup::routes());
    routes
}

/// The segment that names an account in page URLs: its handle, or its DID
/// when it has none.
pub(super) fn account_id(account: &AccountInfo) -> String {
    account
        .handle
        .clone()
        .unwrap_or_else(|| account.did.clone())
}

pub(super) fn account_href(account_id: &str) -> String {
    format!("{ACCOUNT_PATH}/u/{account_id}")
}

fn sign_in_href(login_hint: Option<&str>) -> String {
    match login_hint {
        Some(hint) => format!(
            "{SIGN_IN_PATH}?login_hint={}",
            url::form_urlencoded::byte_serialize(hint.as_bytes()).collect::<String>()
        ),
        None => SIGN_IN_PATH.to_string(),
    }
}

pub(super) fn redirect(href: String) -> Result<Redirect, UiHtml> {
    Ok(Redirect::to(href))
}

/// A session with what the pages need to know about its standing.
struct DeviceSessionInfo {
    linked: DeviceAccount,
    login_required: bool,
}

/// The accounts on this device, freshest authentication first.
async fn device_sessions(
    shared: &SharedOAuthProvider,
    session: &DeviceSession,
    now: u64,
) -> Result<Vec<DeviceSessionInfo>, OAuthError> {
    let mut linked = shared
        .provider
        .store()
        .list_device_accounts(&session.device_id)
        .await?;
    linked.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(linked
        .into_iter()
        .map(|linked| DeviceSessionInfo {
            login_required: shared.provider.check_login_required(&linked, now),
            linked,
        })
        .collect())
}

fn account_cards(sessions: &[DeviceSessionInfo]) -> Vec<AccountCardView> {
    sessions
        .iter()
        .map(|s| AccountCardView::from_account(&s.linked.account, s.login_required))
        .collect()
}

/// What the account manager's sign-in page shows on top of the device.
#[derive(Default)]
struct SignInOptions {
    view: Option<SignInView>,
    identifier: Option<String>,
    error: Option<String>,
    otp_hint: Option<String>,
    otp_error: bool,
}

fn sign_in_page(
    ui: &UiState,
    session: &DeviceSession,
    sessions: &[DeviceSessionInfo],
    options: SignInOptions,
) -> SignInPage {
    let view = options.view.unwrap_or(if sessions.is_empty() {
        SignInView::Form
    } else {
        SignInView::Picker
    });
    let identifier_readonly = matches!(
        view,
        SignInView::ForcedIdentifier | SignInView::ConfirmSelected
    );
    let back_href = match view {
        SignInView::Picker => ui
            .offers_signup()
            .then(|| ACCOUNT_PATH.to_string())
            .unwrap_or_default(),
        _ if !sessions.is_empty() => SIGN_IN_PATH.to_string(),
        _ => ui
            .offers_signup()
            .then(|| ACCOUNT_PATH.to_string())
            .unwrap_or_default(),
    };
    SignInPage {
        shell: (*ui.shell).clone(),
        view,
        subtitle: SignInPage::subtitle_for(view).to_string(),
        client_id: String::new(),
        request_uri: String::new(),
        csrf: session.csrf.clone(),
        identifier: options.identifier.unwrap_or_default(),
        identifier_readonly,
        error: options.error,
        otp_hint: options.otp_hint,
        otp_error: options.otp_error,
        show_remember: false,
        remember_checked: false,
        submit_label: "Sign in".to_string(),
        sessions: account_cards(sessions),
        show_picker: view == SignInView::Picker,
        sign_in_action: SIGN_IN_PATH.to_string(),
        select_action: SELECT_PATH.to_string(),
        another_account_href: format!("{SIGN_IN_PATH}?view=sign-in"),
        signup_href: ui.account_signup_href(),
        forgot_href: Some(super::reset::RESET_PATH.to_string()),
        back_href,
        back_label: "Back".to_string(),
    }
}

pub(super) fn nav_for(
    session: &DeviceSession,
    account: &AccountInfo,
    section: Section,
) -> AccountNav {
    let id = account_id(account);
    AccountNav {
        base_href: account_href(&id),
        items: AccountNav::items_for(&id, section),
        page_title: section.title().to_string(),
        at_base: section == Section::Home,
        account: AccountCardView::from_account(account, false),
        can_switch: true,
        switch_href: SIGN_IN_PATH.to_string(),
        sign_out_action: format!("{}/sign-out", account_href(&id)),
        csrf: session.csrf.clone(),
    }
}

/// The device session and the account the URL names, when the session may
/// act for it: linked to this device, authenticated recently enough, and
/// active unless the page is one a deactivated account may still reach.
pub(super) async fn page_session(
    ui: &UiState,
    shared: &SharedOAuthProvider,
    jar: &CookieJar<'_>,
    info: &OAuthRequestInfo,
    id: &str,
    section: Section,
    now: u64,
) -> Result<(DeviceSession, AccountInfo), Result<Redirect, UiHtml>> {
    let shell = &ui.shell;
    let session = device_session(shell, shared, jar, info, now)
        .await
        .map_err(Err)?;
    let linked = match resolve_linked(shared, &session, id).await {
        Ok(Some(linked)) => linked,
        Ok(None) => return Err(redirect(sign_in_href(Some(id)))),
        Err(error) => return Err(Err(oauth_error_page(shell, error))),
    };
    if shared.provider.check_login_required(&linked, now) {
        return Err(redirect(sign_in_href(Some(id))));
    }
    if linked.account.deactivated && !matches!(section, Section::Home | Section::Manage) {
        return Err(redirect(account_href(&account_id(&linked.account))));
    }
    Ok((session, linked.account))
}

/// The membership row for the account `id` names, checked together with
/// the session secret the request presented.
async fn resolve_linked(
    shared: &SharedOAuthProvider,
    session: &DeviceSession,
    id: &str,
) -> Result<Option<DeviceAccount>, OAuthError> {
    let store = shared.provider.store();
    let did = if id.starts_with("did:") {
        id.to_string()
    } else {
        let wanted = id.trim_start_matches('@').to_ascii_lowercase();
        let Some(linked) = store
            .list_device_accounts(&session.device_id)
            .await?
            .into_iter()
            .find(|linked| {
                linked
                    .account
                    .handle
                    .as_deref()
                    .is_some_and(|handle| handle.eq_ignore_ascii_case(&wanted))
            })
        else {
            return Ok(None);
        };
        linked.account.did
    };
    store
        .get_device_account_for_session(&session.device_id, &session.session_id, &did)
        .await
}

pub(super) fn error_page(
    shell: &crate::ui::shell::PageShell,
    status: Status,
    message: String,
) -> UiHtml {
    render_error(shell, status, message)
}

pub(super) fn cross_site_page(ui: &UiState, back_href: String) -> UiHtml {
    render_page(
        Status::Forbidden,
        &ui.shell,
        &ErrorPage::with_back((*ui.shell).clone(), CROSS_SITE_POST, back_href),
    )
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account")]
pub async fn account_index(
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let session = device_session(shell, shared, jar, &info, now).await?;
    let sessions = device_sessions(shared, &session, now)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    let usable: Vec<&DeviceSessionInfo> = sessions.iter().filter(|s| !s.login_required).collect();
    if let [only] = usable.as_slice() {
        return redirect(account_href(&account_id(&only.linked.account)));
    }
    if sessions.is_empty() {
        if let Some(signup) = ui.account_signup_href() {
            let page = WelcomePage {
                shell: (**shell).clone(),
                csrf: session.csrf.clone(),
                client_id: String::new(),
                request_uri: String::new(),
                signup_href: signup,
                sign_in_href: format!("{SIGN_IN_PATH}?view=sign-in"),
                cancel_action: None,
            };
            return Err(render_page(Status::Ok, shell, &page));
        }
    }
    redirect(SIGN_IN_PATH.to_string())
}

#[derive(FromForm)]
pub struct AccountSignInQuery {
    pub login_hint: Option<String>,
    pub otp_hint: Option<String>,
    pub otp_error: Option<bool>,
    pub auth_error: Option<bool>,
    pub view: Option<String>,
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/sign-in?<query..>")]
pub async fn account_sign_in_page(
    query: AccountSignInQuery,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let session = device_session(shell, shared, jar, &info, now).await?;
    let sessions = device_sessions(shared, &session, now)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    let otp_error = query.otp_error.unwrap_or(false);
    let auth_error = query.auth_error.unwrap_or(false);
    let otp_hint = query.otp_hint.filter(|hint| !hint.is_empty());
    let login_hint = query.login_hint.filter(|hint| !hint.is_empty());
    let gated = otp_error || auth_error || otp_hint.is_some();
    let view = if login_hint.is_some() {
        Some(SignInView::ForcedIdentifier)
    } else if gated || query.view.as_deref() == Some("sign-in") {
        Some(SignInView::Form)
    } else {
        None
    };
    let error = if otp_error {
        Some(CODE_REJECTED.to_string())
    } else if auth_error {
        Some(CREDENTIALS_REJECTED.to_string())
    } else {
        None
    };
    let options = SignInOptions {
        view,
        identifier: login_hint,
        error,
        otp_hint,
        otp_error,
    };
    Err(render_page(
        Status::Ok,
        shell,
        &sign_in_page(ui, &session, &sessions, options),
    ))
}

#[derive(FromForm)]
pub struct AccountSignInFormData {
    pub csrf: String,
    pub identifier: String,
    pub password: String,
    /// Consumed by a second-factor gate in front of this route; ignored here.
    pub email_otp: Option<String>,
}

/// Signs the account in on this device, always remembered: a server-rendered
/// page has nowhere safe to carry an ephemeral proof between visits.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/sign-in", data = "<form>")]
pub async fn account_sign_in(
    form: Form<AccountSignInFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let mut session = device_session(shell, shared, jar, &info, now).await?;
    if info.cross_site {
        return Err(cross_site_page(ui, SIGN_IN_PATH.to_string()));
    }
    let sessions = device_sessions(shared, &session, now)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    let form_again = |error: &str| {
        let options = SignInOptions {
            view: Some(SignInView::Form),
            identifier: Some(form.identifier.clone()),
            error: Some(error.to_string()),
            ..SignInOptions::default()
        };
        render_page(
            Status::Ok,
            shell,
            &sign_in_page(ui, &session, &sessions, options),
        )
    };
    if form.csrf != session.csrf {
        return Err(form_again(SESSION_CHANGED));
    }
    let store = shared.provider.store();
    let account = match store
        .authenticate_account(&form.identifier, &form.password)
        .await
    {
        Ok(Some(account)) => account,
        Ok(None) => return Err(form_again(CREDENTIALS_REJECTED)),
        Err(error) => return Err(oauth_error_page(shell, error)),
    };
    let new_session_id = generate_session_id();
    match store
        .authenticate_device_account(
            &session.device_id,
            &session.session_id,
            &new_session_id,
            &account.did,
            now,
        )
        .await
    {
        Ok(()) => {}
        Err(OAuthError::InvalidRequest(reason)) if reason == "device session changed" => {
            return Err(form_again(SESSION_CHANGED));
        }
        Err(error) => return Err(oauth_error_page(shell, error)),
    }
    crate::metrics::record_login_success("account");
    adopt_session(shared, jar, &mut session, new_session_id);
    redirect(account_href(&account_id(&account)))
}

#[derive(FromForm)]
pub struct SelectFormData {
    pub csrf: String,
    pub did: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/select", data = "<form>")]
pub async fn account_select(
    form: Form<SelectFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let session = device_session(shell, shared, jar, &info, now).await?;
    if info.cross_site {
        return Err(cross_site_page(ui, SIGN_IN_PATH.to_string()));
    }
    let sessions = device_sessions(shared, &session, now)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    if form.csrf != session.csrf {
        let options = SignInOptions {
            error: Some(SESSION_CHANGED.to_string()),
            ..SignInOptions::default()
        };
        return Err(render_page(
            Status::Ok,
            shell,
            &sign_in_page(ui, &session, &sessions, options),
        ));
    }
    let Some(picked) = sessions.iter().find(|s| s.linked.account.did == form.did) else {
        return Err(render_error(
            shell,
            Status::BadRequest,
            "account is not signed in on this device",
        ));
    };
    if picked.login_required {
        let options = SignInOptions {
            view: Some(SignInView::ConfirmSelected),
            identifier: Some(account_id(&picked.linked.account)),
            ..SignInOptions::default()
        };
        return Err(render_page(
            Status::Ok,
            shell,
            &sign_in_page(ui, &session, &sessions, options),
        ));
    }
    redirect(account_href(&account_id(&picked.linked.account)))
}

#[derive(FromForm)]
pub struct CsrfFormData {
    pub csrf: String,
}

/// Signs every account out of this device.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/sign-out-all", data = "<form>")]
pub async fn account_sign_out_all(
    form: Form<CsrfFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let mut session = device_session(shell, shared, jar, &info, now).await?;
    check_form_origin(shell, &info, &session, &form.csrf, ACCOUNT_PATH.to_string())?;
    let store = shared.provider.store();
    let linked = store
        .list_device_accounts(&session.device_id)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    for account in linked {
        store
            .remove_device_account(&session.device_id, &account.account.did)
            .await
            .map_err(|error| oauth_error_page(shell, error))?;
    }
    rotate_session(shared, jar, &mut session)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    redirect(ACCOUNT_PATH.to_string())
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>")]
pub async fn account_home(
    id: &str,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Home, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let nav = nav_for(&session, &account, Section::Home);
    let page = HomePage {
        shell: (*ui.shell).clone(),
        about_href: format!("{}/about", nav.base_href),
        nav,
    };
    Err(render_page(Status::Ok, &ui.shell, &page))
}

/// Signs this account out of this device. Works for a stale session too:
/// leaving is always allowed.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/sign-out", data = "<form>")]
pub async fn account_sign_out(
    id: &str,
    form: Form<CsrfFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let mut session = device_session(shell, shared, jar, &info, now).await?;
    check_form_origin(shell, &info, &session, &form.csrf, account_href(id))?;
    let Some(linked) = resolve_linked(shared, &session, id)
        .await
        .map_err(|error| oauth_error_page(shell, error))?
    else {
        return redirect(ACCOUNT_PATH.to_string());
    };
    shared
        .provider
        .store()
        .remove_device_account(&session.device_id, &linked.account.did)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    rotate_session(shared, jar, &mut session)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    redirect(ACCOUNT_PATH.to_string())
}

fn matches_filter(filter: &str, haystack: &[&str]) -> bool {
    let needle = filter.trim().to_lowercase();
    needle.is_empty()
        || haystack
            .iter()
            .any(|text| text.to_lowercase().contains(&needle))
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/devices?<q>")]
pub async fn account_devices(
    id: &str,
    q: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Devices, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let mut linked = shared
        .provider
        .store()
        .list_account_devices(&account.did)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    linked.sort_by(|a, b| b.device.last_seen_at.cmp(&a.device.last_seen_at));
    let filter = q.unwrap_or_default();
    let total = linked.len();
    let devices: Vec<DeviceRow> = linked
        .into_iter()
        .map(|linked| DeviceRow {
            current: linked.device_id == session.device_id,
            name: browser_name(linked.device.user_agent.as_deref()).unwrap_or_default(),
            ip_address: linked.device.ip_address,
            last_seen: date_ago(now, linked.device.last_seen_at),
            device_id: linked.device_id,
        })
        .filter(|row| matches_filter(&filter, &[&row.name, &row.ip_address]))
        .collect();
    let nav = nav_for(&session, &account, Section::Devices);
    let page = DevicesPage {
        shell: (**shell).clone(),
        apps_href: format!("{}/apps", nav.base_href),
        sign_out_action: format!("{}/devices/sign-out", nav.base_href),
        nav,
        filter,
        total,
        devices,
    };
    Err(render_page(Status::Ok, shell, &page))
}

#[derive(FromForm)]
pub struct DeviceSignOutFormData {
    pub csrf: String,
    pub device_id: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/devices/sign-out", data = "<form>")]
pub async fn account_device_sign_out(
    id: &str,
    form: Form<DeviceSignOutFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Devices, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let devices_href = format!("{}/devices", account_href(&account_id(&account)));
    check_form_origin(shell, &info, &session, &form.csrf, devices_href.clone())?;
    if form.device_id == session.device_id {
        return Err(render_page(
            Status::BadRequest,
            shell,
            &ErrorPage::with_back(
                (**shell).clone(),
                CANNOT_REMOVE_CURRENT_DEVICE,
                devices_href,
            ),
        ));
    }
    shared
        .provider
        .store()
        .remove_device_account(&form.device_id, &account.did)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    redirect(devices_href)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/apps?<q>")]
pub async fn account_apps(
    id: &str,
    q: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Apps, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let mut sessions = shared
        .provider
        .list_account_sessions(&account.did, now)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let nav = nav_for(&session, &account, Section::Apps);
    let filter = q.unwrap_or_default();
    let total = sessions.len();
    let apps: Vec<AppRow> = sessions
        .into_iter()
        .map(|s| AppRow {
            details_href: format!("{}/apps/{}", nav.base_href, s.token_id),
            name: client_app_name(
                &s.client_id,
                s.client_metadata
                    .as_ref()
                    .and_then(|m| m.client_name.as_deref()),
            ),
            identifier: client_identifier(&s.client_id),
            authorized: calendar_date(s.created_at),
            last_accessed: date_ago(now, s.updated_at),
            token_id: s.token_id,
        })
        .filter(|row| matches_filter(&filter, &[&row.name, &row.identifier]))
        .collect();
    let page = AppsPage {
        shell: (**shell).clone(),
        nav,
        filter,
        total,
        apps,
    };
    Err(render_page(Status::Ok, shell, &page))
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/apps/<token_id>")]
pub async fn account_app_details(
    id: &str,
    token_id: &str,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Apps, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let nav = nav_for(&session, &account, Section::Apps);
    let apps_href = format!("{}/apps", nav.base_href);
    let Some(found) = shared
        .provider
        .list_account_sessions(&account.did, now)
        .await
        .map_err(|error| oauth_error_page(shell, error))?
        .into_iter()
        .find(|s| s.token_id == token_id)
    else {
        return Err(render_page(
            Status::NotFound,
            shell,
            &ErrorPage::with_back((**shell).clone(), "Unknown app session", apps_href),
        ));
    };
    let scope = found
        .scope
        .clone()
        .or_else(|| found.client_metadata.as_ref().and_then(|m| m.scope.clone()))
        .unwrap_or_default();
    let scopes: Vec<String> = scope.split_ascii_whitespace().map(String::from).collect();
    let first_party = shared.provider.is_trusted_client(&found.client_id)
        && ui.is_first_party_client(&found.client_id);
    let grouping = permission_groups(
        &scopes,
        &BTreeMap::new(),
        first_party,
        shell.app_name.as_deref(),
    );
    let page = AppDetailsPage {
        shell: (**shell).clone(),
        revoke_action: format!("{apps_href}/revoke"),
        back_href: apps_href,
        nav,
        token_id: found.token_id,
        name: client_app_name(
            &found.client_id,
            found
                .client_metadata
                .as_ref()
                .and_then(|m| m.client_name.as_deref()),
        ),
        identifier: client_identifier(&found.client_id),
        only_atproto: scopes.is_empty() || grouping.only_atproto,
        groups: grouping.groups,
    };
    Err(render_page(Status::Ok, shell, &page))
}

#[derive(FromForm)]
pub struct RevokeFormData {
    pub csrf: String,
    pub token_id: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/apps/revoke", data = "<form>")]
pub async fn account_app_revoke(
    id: &str,
    form: Form<RevokeFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Apps, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let apps_href = format!("{}/apps", account_href(&account_id(&account)));
    check_form_origin(shell, &info, &session, &form.csrf, apps_href.clone())?;
    shared
        .provider
        .revoke_account_token(&account.did, &form.token_id)
        .await
        .map_err(|error| oauth_error_page(shell, error))?;
    redirect(apps_href)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/about")]
pub async fn account_about(
    id: &str,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let shell = &ui.shell;
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::About, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let nav = nav_for(&session, &account, Section::About);
    let page = AboutPage {
        shell: (**shell).clone(),
        handle: nav.account.handle.clone(),
        profile_href: shell.app_url.as_ref().map(|url| {
            format!(
                "{}/profile/{}",
                url.trim_end_matches('/'),
                account_id(&account)
            )
        }),
        nav,
    };
    Err(render_page(Status::Ok, shell, &page))
}

/// Anything else under `/account` is a page that does not exist.
#[rocket::get("/account/<_..>", rank = 20)]
pub fn account_not_found(ui: &State<UiState>) -> UiHtml {
    render_page(
        Status::NotFound,
        &ui.shell,
        &ErrorPage::not_found((*ui.shell).clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrefs_and_filters() {
        assert_eq!(account_href("alice.test"), "/account/u/alice.test");
        assert_eq!(sign_in_href(None), "/account/sign-in");
        assert_eq!(
            sign_in_href(Some("did:plc:a b")),
            "/account/sign-in?login_hint=did%3Aplc%3Aa+b"
        );
        let account = AccountInfo {
            did: "did:plc:a".into(),
            handle: None,
            email: None,
            email_verified: false,
            deactivated: false,
        };
        assert_eq!(account_id(&account), "did:plc:a");
        assert!(matches_filter("", &["anything"]));
        assert!(matches_filter(
            " Saf ",
            &["macOS \u{2022} Safari", "10.0.0.1"]
        ));
        assert!(!matches_filter(
            "windows",
            &["macOS \u{2022} Safari", "10.0.0.1"]
        ));
    }
}
