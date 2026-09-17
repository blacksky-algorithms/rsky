//! Password reset for someone who cannot sign in, and the well-known
//! change-password address that points at it.

use super::manage::{page_message, SOMETHING_WENT_WRONG};
use super::routes::{cross_site_page, SIGN_IN_PATH};
use crate::account_manager::{AccountManager, ResetPasswordOpts};
use crate::apis::com::atproto::server::request_password_reset::request_password_reset_for;
use crate::apis::ApiError;
use crate::oauth::routes::{device_session, OAuthRequestInfo, SESSION_CHANGED};
use crate::oauth::{now_secs, DeviceSession, SharedOAuthProvider};
use crate::rate_limits::{Caller, RateLimits};
use crate::ui::pages::oauth::{ResetPasswordPage, ResetView};
use crate::ui::respond::{render_page, UiHtml};
use crate::ui::UiState;
use rocket::form::Form;
use rocket::http::{CookieJar, Status};
use rocket::response::Redirect;
use rocket::{FromForm, Route, State};
use std::time::Duration;

pub const RESET_PATH: &str = "/account/reset-password";
const NEW_PASSWORD_MIN_LENGTH: usize = 8;
const NEW_PASSWORD_MAX_LENGTH: usize = 256;
/// Failed attempts take at least this long, so timing tells nothing.
const FAILURE_FLOOR: Duration = Duration::from_millis(300);

pub fn routes() -> Vec<Route> {
    rocket::routes![
        reset_page_route,
        reset_request,
        reset_confirm,
        well_known_change_password
    ]
}

fn reset_page(
    ui: &UiState,
    session: &DeviceSession,
    view: ResetView,
    email: String,
    error: Option<String>,
) -> UiHtml {
    let page = ResetPasswordPage {
        shell: (*ui.shell).clone(),
        view,
        csrf: session.csrf.clone(),
        email,
        error,
        request_action: format!("{RESET_PATH}/request"),
        confirm_action: format!("{RESET_PATH}/confirm"),
        request_href: RESET_PATH.to_string(),
        code_href: format!("{RESET_PATH}?view=confirm"),
        back_href: SIGN_IN_PATH.to_string(),
        sign_in_href: SIGN_IN_PATH.to_string(),
    };
    render_page(Status::Ok, &ui.shell, &page)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/reset-password?<email>&<view>")]
pub async fn reset_page_route(
    email: Option<String>,
    view: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let session = device_session(&ui.shell, shared, jar, &info, now_secs()).await?;
    let view = match view.as_deref() {
        Some("confirm") => ResetView::Confirm,
        Some("updated") => ResetView::Updated,
        _ => ResetView::Request,
    };
    Err(reset_page(
        ui,
        &session,
        view,
        email.unwrap_or_default(),
        None,
    ))
}

#[derive(FromForm)]
pub struct ResetRequestFormData {
    pub csrf: String,
    pub email: String,
}

/// Mails a reset code. The answer is the same whether or not the address
/// belongs to an account, and never faster than the failure floor.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/reset-password/request", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn reset_request(
    form: Form<ResetRequestFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Redirect, UiHtml> {
    let session = device_session(&ui.shell, shared, jar, &info, now_secs()).await?;
    if info.cross_site {
        return Err(cross_site_page(ui, RESET_PATH.to_string()));
    }
    let email = form.email.trim().to_lowercase();
    if form.csrf != session.csrf {
        return Err(reset_page(
            ui,
            &session,
            ResetView::Request,
            email,
            Some(SESSION_CHANGED.to_string()),
        ));
    }
    if let Err(error) = limits
        .consume_all(
            &crate::rate_limits::REQUEST_PASSWORD_RESET,
            &caller.ip,
            1,
            caller.bypass,
        )
        .await
    {
        return Err(reset_page(
            ui,
            &session,
            ResetView::Request,
            email,
            Some(page_message(&error)),
        ));
    }
    let started = std::time::Instant::now();
    if let Err(error) = request_password_reset_for(&email, account_manager).await {
        tracing::info!(%error, "password reset requested for an address that could not be served");
    }
    tokio::time::sleep(FAILURE_FLOOR.saturating_sub(started.elapsed())).await;
    Err(reset_page(ui, &session, ResetView::Confirm, email, None))
}

#[derive(FromForm)]
pub struct ResetConfirmFormData {
    pub csrf: String,
    pub code: String,
    pub password: String,
    pub username: Option<String>,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/reset-password/confirm", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn reset_confirm(
    form: Form<ResetConfirmFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Redirect, UiHtml> {
    let session = device_session(&ui.shell, shared, jar, &info, now_secs()).await?;
    if info.cross_site {
        return Err(cross_site_page(ui, RESET_PATH.to_string()));
    }
    let email = form.username.clone().unwrap_or_default();
    let again =
        |error: String| reset_page(ui, &session, ResetView::Confirm, email.clone(), Some(error));
    if form.csrf != session.csrf {
        return Err(again(SESSION_CHANGED.to_string()));
    }
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
    let started = std::time::Instant::now();
    if let Err(error) = account_manager
        .reset_password(ResetPasswordOpts {
            token: form.code.trim().to_string(),
            password: form.password.clone(),
        })
        .await
    {
        tokio::time::sleep(FAILURE_FLOOR.saturating_sub(started.elapsed())).await;
        let message = match ApiError::from(error) {
            ApiError::RuntimeError => SOMETHING_WENT_WRONG.to_string(),
            other => page_message(&other),
        };
        return Err(again(message));
    }
    Err(reset_page(
        ui,
        &session,
        ResetView::Updated,
        String::new(),
        None,
    ))
}

/// <https://www.w3.org/TR/change-password-url/>
#[rocket::get("/.well-known/change-password")]
pub fn well_known_change_password() -> Redirect {
    Redirect::found(RESET_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_well_known_address_points_at_the_reset_page() {
        assert_eq!(RESET_PATH, "/account/reset-password");
    }
}
