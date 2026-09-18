//! The "Account" section of the account manager: email, username and
//! password, each a short sequence of pages built on the same operations
//! the XRPC methods use.

use super::routes::{account_href, account_id, nav_for, page_session, redirect};
use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::{AccountManager, ResetPasswordOpts};
use crate::apis::com::atproto::identity::update_handle::update_handle_for;
use crate::apis::com::atproto::server::confirm_email::confirm_email_for;
use crate::apis::com::atproto::server::request_email_confirmation::request_email_confirmation_for;
use crate::apis::com::atproto::server::request_email_update::request_email_update_for;
use crate::apis::com::atproto::server::request_password_reset::request_password_reset_for;
use crate::apis::com::atproto::server::update_email::update_email_for;
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::models::models::EmailTokenPurpose;
use crate::oauth::routes::{check_form_origin, OAuthRequestInfo};
use crate::oauth::{now_secs, DeviceSession, SharedOAuthProvider};
use crate::rate_limits::{Caller, RateLimits};
use crate::ui::pages::account::{
    EmailPage, EmailStep, HandleMode, HandlePage, ManagePage, PasswordPage, PasswordStep, Section,
    SettingRow,
};
use crate::ui::respond::{render_page, UiHtml};
use crate::ui::UiState;
use crate::{SharedIdResolver, SharedSequencer};
use rocket::form::Form;
use rocket::http::{CookieJar, Status};
use rocket::response::Redirect;
use rocket::{FromForm, Route, State};
use rsky_oauth::store::AccountInfo;

const NEW_PASSWORD_MIN_LENGTH: usize = 8;
const NEW_PASSWORD_MAX_LENGTH: usize = 256;
pub(super) const SOMETHING_WENT_WRONG: &str = "Something went wrong";
const INVALID_PASSWORD: &str = "Invalid password";

pub fn routes() -> Vec<Route> {
    rocket::routes![
        manage_page,
        email_page_route,
        email_request,
        email_confirm,
        email_verify_page,
        email_verify_request,
        email_verify,
        handle_page_route,
        handle_update,
        password_page_route,
        password_request,
        password_update,
    ]
}

/// The wording a page shows for an operation's error: the token and
/// request messages the reference uses; anything else is logged and
/// reported generically.
pub(super) fn page_message(error: &ApiError) -> String {
    match error {
        ApiError::InvalidToken(message) => message.clone(),
        ApiError::ExpiredEmailToken | ApiError::ExpiredToken => "Token is expired".to_string(),
        ApiError::InvalidRequest(message) => message.clone(),
        ApiError::InvalidEmail => {
            "This email address is not supported, please use a different email.".to_string()
        }
        ApiError::AuthRequiredError(message) => message.clone(),
        other => {
            tracing::warn!(error = %other, "account page operation failed");
            SOMETHING_WENT_WRONG.to_string()
        }
    }
}

fn manage_href(account: &AccountInfo) -> String {
    format!("{}/manage", account_href(&account_id(account)))
}

/// The notices the manage page shows after a completed change.
fn notice_for(key: Option<&str>) -> Option<String> {
    match key? {
        "email" => Some("Your email address has been updated.".to_string()),
        "verified" => Some("Your email address has been verified.".to_string()),
        "handle" => Some("Your username has been updated.".to_string()),
        "password" => Some("Your password has been updated.".to_string()),
        "reactivated" => Some("Your account has been reactivated.".to_string()),
        _ => None,
    }
}

/// What the account manager knows about the account beyond the OAuth
/// store's view: its address and whether it is confirmed.
async fn account_details(
    account_manager: &AccountManager,
    did: &str,
) -> Result<(Option<String>, bool), ApiError> {
    let account = account_manager
        .get_account(
            did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: None,
            }),
        )
        .await?
        .ok_or_else(|| ApiError::InvalidRequest("Account not found".to_string()))?;
    Ok((account.email, account.email_confirmed_at.is_some()))
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/manage?<notice>")]
pub async fn manage_page(
    id: &str,
    notice: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let (email, email_confirmed) = account_details(account_manager, &account.did)
        .await
        .map_err(|error| {
            super::routes::error_page(&ui.shell, Status::BadRequest, page_message(&error))
        })?;
    let base = manage_href(&account);
    let nav = nav_for(&session, &account, Section::Manage);
    let mut rows = Vec::new();
    if !account.deactivated {
        rows.push(SettingRow {
            href: format!("{base}/email"),
            title: "Email address",
            value: email.clone().unwrap_or_default(),
            icon: "mail",
            destructive: false,
        });
        rows.push(SettingRow {
            href: format!("{base}/handle"),
            title: "Username",
            value: nav.account.handle.clone(),
            icon: "at-sign",
            destructive: false,
        });
        if email.is_some() {
            rows.push(SettingRow {
                href: format!("{base}/password"),
                title: "Password",
                value: String::new(),
                icon: "lock",
                destructive: false,
            });
        }
    }
    rows.push(if account.deactivated {
        SettingRow {
            href: format!("{base}/reactivate"),
            title: "Reactivate account",
            value: String::new(),
            icon: "snowflake",
            destructive: false,
        }
    } else {
        SettingRow {
            href: format!("{base}/deactivate"),
            title: "Deactivate account",
            value: String::new(),
            icon: "snowflake",
            destructive: true,
        }
    });
    rows.push(SettingRow {
        href: format!("{base}/delete"),
        title: "Delete account",
        value: String::new(),
        icon: "trash",
        destructive: true,
    });
    let page = ManagePage {
        shell: (*ui.shell).clone(),
        nav,
        unverified_email: email.filter(|_| !email_confirmed && !account.deactivated),
        verify_href: format!("{base}/email/verify"),
        deactivated: account.deactivated,
        rows,
        notice: notice_for(notice.as_deref()),
    };
    Err(render_page(Status::Ok, &ui.shell, &page))
}

fn email_page(
    ui: &UiState,
    session: &DeviceSession,
    account: &AccountInfo,
    current_email: Option<String>,
    step: EmailStep,
    new_email: String,
    error: Option<String>,
) -> UiHtml {
    let base = manage_href(account);
    let page = EmailPage {
        shell: (*ui.shell).clone(),
        nav: nav_for(session, account, Section::Manage),
        step,
        current_email,
        new_email,
        error,
        request_action: format!("{base}/email/request"),
        confirm_action: format!("{base}/email/confirm"),
        verify_request_action: format!("{base}/email/verify/request"),
        verify_action: format!("{base}/email/verify"),
        verify_code_href: format!("{base}/email/verify?step=code"),
        cancel_href: base,
    };
    render_page(Status::Ok, &ui.shell, &page)
}

/// Everything an email page needs before it can decide anything.
struct EmailContext {
    session: DeviceSession,
    account: AccountInfo,
    current_email: Option<String>,
}

async fn email_context(
    ui: &UiState,
    shared: &SharedOAuthProvider,
    account_manager: &AccountManager,
    jar: &CookieJar<'_>,
    info: &OAuthRequestInfo,
    id: &str,
    now: u64,
) -> Result<EmailContext, Result<Redirect, UiHtml>> {
    let (session, account) = page_session(ui, shared, jar, info, id, Section::Manage, now).await?;
    if account.deactivated {
        return Err(redirect(manage_href(&account)));
    }
    let (current_email, _) = account_details(account_manager, &account.did)
        .await
        .map_err(|error| {
            Err(super::routes::error_page(
                &ui.shell,
                Status::BadRequest,
                page_message(&error),
            ))
        })?;
    Ok(EmailContext {
        session,
        account,
        current_email,
    })
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/manage/email")]
pub async fn email_page_route(
    id: &str,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
) -> Result<Redirect, UiHtml> {
    let ctx = match email_context(ui, shared, account_manager, jar, &info, id, now_secs()).await {
        Ok(ctx) => ctx,
        Err(answer) => return answer,
    };
    Err(email_page(
        ui,
        &ctx.session,
        &ctx.account,
        ctx.current_email,
        EmailStep::Choose,
        String::new(),
        None,
    ))
}

#[derive(FromForm)]
pub struct EmailRequestFormData {
    pub csrf: String,
    pub new_email: String,
}

/// Step one of an email change: a confirmed current address gets a code
/// first; an unconfirmed one is replaced right away and the new address
/// is offered a verification code.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/email/request", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn email_request(
    id: &str,
    form: Form<EmailRequestFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let ctx = match email_context(ui, shared, account_manager, jar, &info, id, now).await {
        Ok(ctx) => ctx,
        Err(answer) => return answer,
    };
    check_form_origin(
        &ui.shell,
        &info,
        &ctx.session,
        &form.csrf,
        manage_href(&ctx.account),
    )?;
    let new_email = form.new_email.trim().to_lowercase();
    let again = |step: EmailStep, error: String| {
        email_page(
            ui,
            &ctx.session,
            &ctx.account,
            ctx.current_email.clone(),
            step,
            new_email.clone(),
            Some(error),
        )
    };
    if !mailchecker::is_valid(&new_email) {
        return Err(again(
            EmailStep::Choose,
            page_message(&ApiError::InvalidEmail),
        ));
    }
    if let Err(error) = limits
        .consume_all(
            &crate::rate_limits::REQUEST_EMAIL_UPDATE,
            &ctx.account.did,
            1,
            caller.bypass,
        )
        .await
    {
        return Err(again(EmailStep::Choose, page_message(&error)));
    }
    let token_required = match request_email_update_for(&ctx.account.did, account_manager).await {
        Ok(required) => required,
        Err(error) => {
            tracing::warn!(%error, "email update request failed");
            return Err(again(EmailStep::Choose, SOMETHING_WENT_WRONG.to_string()));
        }
    };
    if token_required {
        return Err(email_page(
            ui,
            &ctx.session,
            &ctx.account,
            ctx.current_email.clone(),
            EmailStep::Token,
            new_email,
            None,
        ));
    }
    if let Err(error) = update_email_for(
        ctx.account.did.clone(),
        new_email.clone(),
        None,
        account_manager,
    )
    .await
    {
        return Err(again(EmailStep::Choose, page_message(&error)));
    }
    Err(email_page(
        ui,
        &ctx.session,
        &ctx.account,
        Some(new_email.clone()),
        EmailStep::VerifyRequest,
        new_email,
        None,
    ))
}

#[derive(FromForm)]
pub struct EmailConfirmFormData {
    pub csrf: String,
    pub new_email: String,
    pub code: String,
}

/// Step two: the code sent to the current address proves the change.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/email/confirm", data = "<form>")]
pub async fn email_confirm(
    id: &str,
    form: Form<EmailConfirmFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
) -> Result<Redirect, UiHtml> {
    let ctx = match email_context(ui, shared, account_manager, jar, &info, id, now_secs()).await {
        Ok(ctx) => ctx,
        Err(answer) => return answer,
    };
    check_form_origin(
        &ui.shell,
        &info,
        &ctx.session,
        &form.csrf,
        manage_href(&ctx.account),
    )?;
    let new_email = form.new_email.trim().to_lowercase();
    if let Err(error) = update_email_for(
        ctx.account.did.clone(),
        new_email.clone(),
        Some(form.code.trim().to_string()),
        account_manager,
    )
    .await
    {
        return Err(email_page(
            ui,
            &ctx.session,
            &ctx.account,
            ctx.current_email,
            EmailStep::Token,
            new_email,
            Some(page_message(&error)),
        ));
    }
    Err(email_page(
        ui,
        &ctx.session,
        &ctx.account,
        Some(new_email.clone()),
        EmailStep::VerifyRequest,
        new_email,
        None,
    ))
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/manage/email/verify?<step>")]
pub async fn email_verify_page(
    id: &str,
    step: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
) -> Result<Redirect, UiHtml> {
    let ctx = match email_context(ui, shared, account_manager, jar, &info, id, now_secs()).await {
        Ok(ctx) => ctx,
        Err(answer) => return answer,
    };
    let Some(email) = ctx.current_email.clone() else {
        return redirect(manage_href(&ctx.account));
    };
    let step = if step.as_deref() == Some("code") {
        EmailStep::Verify
    } else {
        EmailStep::VerifyRequest
    };
    Err(email_page(
        ui,
        &ctx.session,
        &ctx.account,
        ctx.current_email,
        step,
        email,
        None,
    ))
}

#[derive(FromForm)]
pub struct CsrfFormData {
    pub csrf: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/email/verify/request", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn email_verify_request(
    id: &str,
    form: Form<CsrfFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Redirect, UiHtml> {
    let ctx = match email_context(ui, shared, account_manager, jar, &info, id, now_secs()).await {
        Ok(ctx) => ctx,
        Err(answer) => return answer,
    };
    check_form_origin(
        &ui.shell,
        &info,
        &ctx.session,
        &form.csrf,
        manage_href(&ctx.account),
    )?;
    let Some(email) = ctx.current_email.clone() else {
        return redirect(manage_href(&ctx.account));
    };
    let error = match limits
        .consume_all(
            &crate::rate_limits::REQUEST_EMAIL_CONFIRMATION,
            &ctx.account.did,
            1,
            caller.bypass,
        )
        .await
    {
        Ok(()) => request_email_confirmation_for(&ctx.account.did, account_manager)
            .await
            .err()
            .map(|error| {
                tracing::warn!(%error, "email confirmation request failed");
                SOMETHING_WENT_WRONG.to_string()
            }),
        Err(error) => Some(page_message(&error)),
    };
    let step = if error.is_some() {
        EmailStep::VerifyRequest
    } else {
        EmailStep::Verify
    };
    Err(email_page(
        ui,
        &ctx.session,
        &ctx.account,
        ctx.current_email,
        step,
        email,
        error,
    ))
}

#[derive(FromForm)]
pub struct EmailVerifyFormData {
    pub csrf: String,
    pub code: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/email/verify", data = "<form>")]
pub async fn email_verify(
    id: &str,
    form: Form<EmailVerifyFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
) -> Result<Redirect, UiHtml> {
    let ctx = match email_context(ui, shared, account_manager, jar, &info, id, now_secs()).await {
        Ok(ctx) => ctx,
        Err(answer) => return answer,
    };
    check_form_origin(
        &ui.shell,
        &info,
        &ctx.session,
        &form.csrf,
        manage_href(&ctx.account),
    )?;
    let Some(email) = ctx.current_email.clone() else {
        return redirect(manage_href(&ctx.account));
    };
    if let Err(error) =
        confirm_email_for(&ctx.account.did, &email, form.code.trim(), account_manager).await
    {
        return Err(email_page(
            ui,
            &ctx.session,
            &ctx.account,
            ctx.current_email,
            EmailStep::Verify,
            email,
            Some(page_message(&error)),
        ));
    }
    redirect(format!("{}?notice=verified", manage_href(&ctx.account)))
}

/// The handle split into the parts the default form edits: the segment
/// and the domain it sits on, when that domain is one of this server's.
fn split_handle<'a>(handle: &'a str, domains: &'a [String]) -> Option<(&'a str, &'a str)> {
    domains
        .iter()
        .find(|domain| handle.ends_with(domain.as_str()) && handle.len() > domain.len())
        .map(|domain| (&handle[..handle.len() - domain.len()], domain.as_str()))
}

#[allow(clippy::too_many_arguments)]
fn handle_page(
    ui: &UiState,
    cfg: &ServerConfig,
    session: &DeviceSession,
    account: &AccountInfo,
    mode: HandleMode,
    typed: Option<(String, String)>,
    custom: Option<String>,
    error: Option<String>,
) -> UiHtml {
    let base = manage_href(account);
    let domains = cfg.identity.service_handle_domains.clone();
    let handle = account.handle.clone().unwrap_or_default();
    let split = split_handle(&handle, &domains);
    let (segment, selected_domain) = typed.unwrap_or_else(|| {
        split
            .map(|(segment, domain)| (segment.to_string(), domain.to_string()))
            .unwrap_or_else(|| (String::new(), domains.first().cloned().unwrap_or_default()))
    });
    let custom = custom.unwrap_or_else(|| {
        if split.is_none() {
            handle.clone()
        } else {
            String::new()
        }
    });
    let page = HandlePage {
        shell: (*ui.shell).clone(),
        nav: nav_for(session, account, Section::Manage),
        mode,
        did: account.did.clone(),
        segment,
        domains,
        selected_domain,
        mailto_href: HandlePage::instructions_mailto(&custom, &account.did),
        custom,
        error,
        default_href: format!("{base}/handle?mode=default"),
        custom_href: format!("{base}/handle?mode=custom"),
        submit_action: format!("{base}/handle"),
        cancel_href: base,
    };
    render_page(Status::Ok, &ui.shell, &page)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/manage/handle?<mode>")]
pub async fn handle_page_route(
    id: &str,
    mode: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    cfg: &State<ServerConfig>,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    if account.deactivated {
        return redirect(manage_href(&account));
    }
    let mode = match mode.as_deref() {
        Some("default") if !cfg.identity.service_handle_domains.is_empty() => HandleMode::Default,
        Some("custom") => HandleMode::Custom,
        _ => HandleMode::Choose,
    };
    Err(handle_page(
        ui, cfg, &session, &account, mode, None, None, None,
    ))
}

#[derive(FromForm)]
pub struct HandleFormData {
    pub csrf: String,
    pub mode: String,
    pub handle: Option<String>,
    pub domain: Option<String>,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/handle", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn handle_update(
    id: &str,
    form: Form<HandleFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    cfg: &State<ServerConfig>,
    account_manager: &State<AccountManager>,
    sequencer: &State<SharedSequencer>,
    id_resolver: &State<SharedIdResolver>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    if account.deactivated {
        return redirect(manage_href(&account));
    }
    check_form_origin(
        &ui.shell,
        &info,
        &session,
        &form.csrf,
        manage_href(&account),
    )?;
    let domains = &cfg.identity.service_handle_domains;
    let (mode, handle, typed, custom) = if form.mode == "custom" {
        let domain = form
            .domain
            .clone()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        (HandleMode::Custom, domain.clone(), None, Some(domain))
    } else {
        let segment = form
            .handle
            .clone()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let domain = form
            .domain
            .clone()
            .filter(|domain| domains.contains(domain))
            .or_else(|| domains.first().cloned())
            .unwrap_or_default();
        (
            HandleMode::Default,
            format!("{segment}{domain}"),
            Some((segment, domain)),
            None,
        )
    };
    let again = |error: String| {
        handle_page(
            ui,
            cfg,
            &session,
            &account,
            mode,
            typed.clone(),
            custom.clone(),
            Some(error),
        )
    };
    if let Err(error) = limits
        .consume_all(
            &crate::rate_limits::UPDATE_HANDLE,
            &account.did,
            1,
            caller.bypass,
        )
        .await
    {
        return Err(again(page_message(&error)));
    }
    if let Err(error) = update_handle_for(
        account.did.clone(),
        handle,
        sequencer,
        cfg,
        id_resolver,
        account_manager,
    )
    .await
    {
        let message = error.to_string();
        let message = if message.starts_with("Handle already taken") {
            "Handle already taken".to_string()
        } else {
            message
        };
        return Err(again(message));
    }
    // the handle in the URL just changed; the manage page is looked up
    // by DID so the redirect always lands
    redirect(format!(
        "{}/manage?notice=handle",
        account_href(&account.did)
    ))
}

fn password_page(
    ui: &UiState,
    session: &DeviceSession,
    account: &AccountInfo,
    step: PasswordStep,
    error: Option<String>,
) -> UiHtml {
    let base = manage_href(account);
    let page = PasswordPage {
        shell: (*ui.shell).clone(),
        nav: nav_for(session, account, Section::Manage),
        step,
        error,
        request_action: format!("{base}/password/request"),
        confirm_action: format!("{base}/password"),
        code_href: format!("{base}/password?step=code"),
        cancel_href: base,
    };
    render_page(Status::Ok, &ui.shell, &page)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/manage/password?<step>")]
pub async fn password_page_route(
    id: &str,
    step: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    if account.deactivated {
        return redirect(manage_href(&account));
    }
    let step = if step.as_deref() == Some("code") {
        PasswordStep::Confirm
    } else {
        PasswordStep::Request
    };
    Err(password_page(ui, &session, &account, step, None))
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/password/request", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn password_request(
    id: &str,
    form: Form<CsrfFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    check_form_origin(
        &ui.shell,
        &info,
        &session,
        &form.csrf,
        manage_href(&account),
    )?;
    let (email, _) = match account_details(account_manager, &account.did).await {
        Ok(details) => details,
        Err(error) => {
            return Err(password_page(
                ui,
                &session,
                &account,
                PasswordStep::Request,
                Some(page_message(&error)),
            ))
        }
    };
    let Some(email) = email else {
        return redirect(manage_href(&account));
    };
    let error = match limits
        .consume_all(
            &crate::rate_limits::REQUEST_PASSWORD_RESET,
            &caller.ip,
            1,
            caller.bypass,
        )
        .await
    {
        Ok(()) => request_password_reset_for(&email, account_manager)
            .await
            .err()
            .map(|error| {
                tracing::warn!(%error, "password reset request failed");
                SOMETHING_WENT_WRONG.to_string()
            }),
        Err(error) => Some(page_message(&error)),
    };
    let step = if error.is_some() {
        PasswordStep::Request
    } else {
        PasswordStep::Confirm
    };
    Err(password_page(ui, &session, &account, step, error))
}

#[derive(FromForm)]
pub struct PasswordFormData {
    pub csrf: String,
    pub code: String,
    pub current_password: String,
    pub password: String,
}

/// The new password takes the mailed code and, stricter than the
/// reference, the current password.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/password", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn password_update(
    id: &str,
    form: Form<PasswordFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    check_form_origin(
        &ui.shell,
        &info,
        &session,
        &form.csrf,
        manage_href(&account),
    )?;
    let again =
        |error: String| password_page(ui, &session, &account, PasswordStep::Confirm, Some(error));
    if form.password.len() < NEW_PASSWORD_MIN_LENGTH
        || form.password.len() > NEW_PASSWORD_MAX_LENGTH
    {
        return Err(again("Invalid password length.".to_string()));
    }
    if let Err(error) = limits
        .consume_all(
            &crate::rate_limits::RESET_PASSWORD,
            &caller.ip,
            1,
            caller.bypass,
        )
        .await
    {
        return Err(again(page_message(&error)));
    }
    match account_manager
        .verify_account_password(&account.did, &form.current_password)
        .await
    {
        Ok(true) => {}
        Ok(false) => return Err(again(INVALID_PASSWORD.to_string())),
        Err(error) => return Err(again(page_message(&ApiError::from(error)))),
    }
    let code = form.code.trim().to_string();
    if let Err(error) = account_manager
        .assert_valid_email_token(&account.did, EmailTokenPurpose::ResetPassword, &code)
        .await
    {
        return Err(again(page_message(&ApiError::from(error))));
    }
    if let Err(error) = account_manager
        .reset_password(ResetPasswordOpts {
            token: code,
            password: form.password.clone(),
        })
        .await
    {
        return Err(again(page_message(&ApiError::from(error))));
    }
    redirect(format!("{}?notice=password", manage_href(&account)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_messages_and_handle_splitting() {
        assert_eq!(notice_for(None), None);
        assert_eq!(notice_for(Some("nope")), None);
        assert!(notice_for(Some("email")).unwrap().contains("email"));
        assert!(notice_for(Some("verified")).unwrap().contains("verified"));
        assert!(notice_for(Some("handle")).unwrap().contains("username"));
        assert!(notice_for(Some("password")).unwrap().contains("password"));
        assert!(notice_for(Some("reactivated"))
            .unwrap()
            .contains("reactivated"));

        assert_eq!(
            page_message(&ApiError::InvalidToken("Token is invalid".into())),
            "Token is invalid"
        );
        assert_eq!(
            page_message(&ApiError::ExpiredEmailToken),
            "Token is expired"
        );
        assert_eq!(page_message(&ApiError::ExpiredToken), "Token is expired");
        assert_eq!(
            page_message(&ApiError::InvalidRequest("nope".into())),
            "nope"
        );
        assert!(page_message(&ApiError::InvalidEmail).contains("not supported"));
        assert_eq!(page_message(&ApiError::AuthRequiredError("x".into())), "x");
        assert_eq!(page_message(&ApiError::RuntimeError), SOMETHING_WENT_WRONG);

        let domains = vec![".pds.test".to_string(), ".other.test".to_string()];
        assert_eq!(
            split_handle("alice.pds.test", &domains),
            Some(("alice", ".pds.test"))
        );
        assert_eq!(
            split_handle("bob.other.test", &domains),
            Some(("bob", ".other.test"))
        );
        assert_eq!(split_handle("alice.com", &domains), None);
        assert_eq!(split_handle(".pds.test", &domains), None);
    }
}
