//! Creating an account from the pages: two steps, on the account manager
//! or inside an authorization request, on the same operation the XRPC
//! method uses.

use super::manage::page_message;
use super::routes::{account_href, redirect, SIGN_IN_PATH};
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::server::create_account::create_account_for;
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::lifecycle::LifecycleStore;
use crate::oauth::routes::{adopt_session, device_session, OAuthRequestInfo, SESSION_CHANGED};
use crate::oauth::{now_secs, DeviceSession, SharedOAuthProvider};
use crate::rate_limits::{Caller, RateLimits};
use crate::ui::pages::oauth::{SignUpPage, SignUpStep};
use crate::ui::respond::{render_page, UiHtml};
use crate::ui::{SignUp, UiState};
use crate::{SharedIdResolver, SharedSequencer};
use rocket::form::Form;
use rocket::http::{CookieJar, Status};
use rocket::response::Redirect;
use rocket::{FromForm, Route, State};
use rsky_lexicon::com::atproto::server::CreateAccountInput;
use rsky_oauth::generate_session_id;

pub const SIGN_UP_PATH: &str = "/account/sign-up";
const NEW_PASSWORD_MIN_LENGTH: usize = 8;
const NEW_PASSWORD_MAX_LENGTH: usize = 256;

pub fn routes() -> Vec<Route> {
    rocket::routes![sign_up_page_route, sign_up]
}

/// Where the sign-up lives: on its own, or inside a request.
pub(crate) struct SignUpContext {
    pub client_id: String,
    pub request_uri: String,
    pub submit_action: String,
    /// Where "Back" on the first step goes
    pub back_href: String,
}

/// What the person typed, kept across the steps.
#[derive(Clone, Default)]
pub(crate) struct SignUpValues {
    pub segment: String,
    pub domain: String,
    pub invite_code: String,
    pub email: String,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sign_up_page(
    ui: &UiState,
    cfg: &ServerConfig,
    csrf: &str,
    ctx: &SignUpContext,
    step: SignUpStep,
    values: &SignUpValues,
    error: Option<String>,
) -> UiHtml {
    let domains = cfg.identity.service_handle_domains.clone();
    let selected_domain = if domains.contains(&values.domain) {
        values.domain.clone()
    } else {
        domains.first().cloned().unwrap_or_default()
    };
    let (tos_href, privacy_href) = (
        cfg.service.terms_of_service_url.clone(),
        cfg.service.privacy_policy_url.clone(),
    );
    let back_href = match step {
        SignUpStep::Handle => ctx.back_href.clone(),
        SignUpStep::Credentials => match (ctx.client_id.is_empty(), ctx.request_uri.is_empty()) {
            (true, true) => format!("{SIGN_UP_PATH}?handle={}", values.segment),
            _ => format!(
                "{}&view=sign-up&handle={}",
                crate::oauth::routes::authorize_href(&ctx.client_id, &ctx.request_uri, None),
                values.segment
            ),
        },
    };
    let page = SignUpPage {
        shell: (*ui.shell).clone(),
        step,
        csrf: csrf.to_string(),
        client_id: ctx.client_id.clone(),
        request_uri: ctx.request_uri.clone(),
        domains,
        segment: values.segment.clone(),
        selected_domain,
        invite_required: cfg.invites.required,
        invite_code: values.invite_code.clone(),
        email: values.email.clone(),
        error,
        submit_action: ctx.submit_action.clone(),
        back_href,
        tos_href,
        privacy_href,
    };
    render_page(Status::Ok, &ui.shell, &page)
}

/// The sign-up form as posted, by either surface.
#[derive(FromForm)]
pub struct SignUpFormData {
    pub csrf: String,
    pub step: String,
    pub handle: String,
    pub domain: Option<String>,
    pub invite_code: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
    pub client_id: Option<String>,
    pub request_uri: Option<String>,
}

impl SignUpFormData {
    pub(crate) fn values(&self) -> SignUpValues {
        SignUpValues {
            segment: self.handle.trim().to_lowercase(),
            domain: self.domain.clone().unwrap_or_default(),
            invite_code: self
                .invite_code
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
            email: self.email.clone().unwrap_or_default().trim().to_lowercase(),
        }
    }
}

/// Everything creating the account needs from the server.
pub(crate) struct SignUpServices<'a> {
    pub cfg: &'a State<ServerConfig>,
    pub account_manager: &'a State<AccountManager>,
    pub sequencer: &'a State<SharedSequencer>,
    pub blobstore_factory: &'a State<BlobstoreFactory>,
    pub actor_store: &'a State<ActorStore>,
    pub id_resolver: &'a State<SharedIdResolver>,
    pub lifecycle_store: &'a State<LifecycleStore>,
    pub limits: &'a State<RateLimits>,
    pub caller: &'a Caller,
}

/// Runs the second step: creates the account and signs the device in to
/// it. Failures come back as the message the form shows.
pub(crate) async fn create_and_sign_in(
    services: &SignUpServices<'_>,
    shared: &SharedOAuthProvider,
    jar: &CookieJar<'_>,
    session: &mut DeviceSession,
    values: &SignUpValues,
    password: &str,
    now: u64,
) -> Result<String, String> {
    if password.len() < NEW_PASSWORD_MIN_LENGTH || password.len() > NEW_PASSWORD_MAX_LENGTH {
        return Err("Invalid password length.".to_string());
    }
    let domains = &services.cfg.identity.service_handle_domains;
    let domain = domains
        .iter()
        .find(|domain| **domain == values.domain)
        .or_else(|| domains.first())
        .cloned()
        .unwrap_or_default();
    services
        .limits
        .consume_all(
            &crate::rate_limits::CREATE_ACCOUNT,
            &services.caller.ip,
            1,
            services.caller.bypass,
        )
        .await
        .map_err(|error| page_message(&error))?;
    let input = CreateAccountInput {
        email: Some(values.email.clone()),
        handle: format!("{}{domain}", values.segment),
        did: None,
        invite_code: (!values.invite_code.is_empty()).then(|| values.invite_code.clone()),
        verification_code: None,
        verification_phone: None,
        password: Some(password.to_string()),
        recovery_key: None,
        plc_op: None,
    };
    let created = create_account_for(
        input,
        None,
        false,
        services.sequencer,
        services.blobstore_factory,
        services.cfg,
        services.id_resolver,
        services.account_manager,
        services.actor_store,
        services.lifecycle_store,
    )
    .await
    .map_err(|error| signup_message(&error))?;
    let new_session_id = generate_session_id();
    shared
        .provider
        .store()
        .authenticate_device_account(
            &session.device_id,
            &session.session_id,
            &new_session_id,
            &created.did,
            now,
        )
        .await
        .map_err(|error| match error {
            rsky_oauth::OAuthError::InvalidRequest(reason)
                if reason == "device session changed" =>
            {
                SESSION_CHANGED.to_string()
            }
            other => page_message(&ApiError::RuntimeError).replace(
                "Something went wrong",
                &format!("Something went wrong ({other})"),
            ),
        })?;
    adopt_session(shared, jar, session, new_session_id);
    crate::metrics::record_login_success("account");
    Ok(created.handle)
}

/// The reference's wording for what can go wrong creating an account.
fn signup_message(error: &ApiError) -> String {
    match error {
        ApiError::HandleNotAvailable => "Handle already taken".to_string(),
        ApiError::InvalidHandle => "Invalid handle".to_string(),
        ApiError::UnsupportedDomain => "Unsupported domain".to_string(),
        ApiError::EmailNotAvailable => "Email already taken".to_string(),
        ApiError::InvalidInviteCode => "This invite code is invalid.".to_string(),
        ApiError::InvalidPassword => "Invalid password".to_string(),
        other => page_message(other),
    }
}

fn account_context() -> SignUpContext {
    SignUpContext {
        client_id: String::new(),
        request_uri: String::new(),
        submit_action: SIGN_UP_PATH.to_string(),
        back_href: "/account".to_string(),
    }
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/sign-up?<handle>")]
pub async fn sign_up_page_route(
    handle: Option<String>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    cfg: &State<ServerConfig>,
) -> Result<Redirect, UiHtml> {
    if ui.signup != SignUp::Internal {
        return redirect(SIGN_IN_PATH.to_string());
    }
    let session = device_session(&ui.shell, shared, jar, &info, now_secs()).await?;
    let values = SignUpValues {
        segment: handle.unwrap_or_default(),
        ..SignUpValues::default()
    };
    Err(sign_up_page(
        ui,
        cfg,
        &session.csrf,
        &account_context(),
        SignUpStep::Handle,
        &values,
        None,
    ))
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/sign-up", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn sign_up(
    form: Form<SignUpFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    cfg: &State<ServerConfig>,
    account_manager: &State<AccountManager>,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    id_resolver: &State<SharedIdResolver>,
    lifecycle_store: &State<LifecycleStore>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Redirect, UiHtml> {
    if ui.signup != SignUp::Internal {
        return redirect(SIGN_IN_PATH.to_string());
    }
    let now = now_secs();
    let mut session = device_session(&ui.shell, shared, jar, &info, now).await?;
    if info.cross_site {
        return Err(super::routes::cross_site_page(ui, SIGN_UP_PATH.to_string()));
    }
    let ctx = account_context();
    let values = form.values();
    if form.csrf != session.csrf {
        return Err(sign_up_page(
            ui,
            cfg,
            &session.csrf,
            &ctx,
            SignUpStep::Handle,
            &values,
            Some(SESSION_CHANGED.to_string()),
        ));
    }
    if form.step != "credentials" {
        return Err(sign_up_page(
            ui,
            cfg,
            &session.csrf,
            &ctx,
            SignUpStep::Credentials,
            &values,
            None,
        ));
    }
    let services = SignUpServices {
        cfg,
        account_manager,
        sequencer,
        blobstore_factory,
        actor_store,
        id_resolver,
        lifecycle_store,
        limits,
        caller: &caller,
    };
    match create_and_sign_in(
        &services,
        shared,
        jar,
        &mut session,
        &values,
        form.password.as_deref().unwrap_or_default(),
        now,
    )
    .await
    {
        Ok(handle) => redirect(account_href(&handle)),
        Err(message) => Err(sign_up_page(
            ui,
            cfg,
            &session.csrf,
            &ctx,
            SignUpStep::Credentials,
            &values,
            Some(message),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signup_messages_follow_the_reference() {
        assert_eq!(
            signup_message(&ApiError::HandleNotAvailable),
            "Handle already taken"
        );
        assert_eq!(signup_message(&ApiError::InvalidHandle), "Invalid handle");
        assert_eq!(
            signup_message(&ApiError::UnsupportedDomain),
            "Unsupported domain"
        );
        assert_eq!(
            signup_message(&ApiError::EmailNotAvailable),
            "Email already taken"
        );
        assert_eq!(
            signup_message(&ApiError::InvalidInviteCode),
            "This invite code is invalid."
        );
        assert_eq!(
            signup_message(&ApiError::InvalidPassword),
            "Invalid password"
        );
        assert_eq!(
            signup_message(&ApiError::RuntimeError),
            "Something went wrong"
        );
    }

    #[test]
    fn form_values_are_normalised() {
        let form = SignUpFormData {
            csrf: "c".into(),
            step: "handle".into(),
            handle: " Alice ".into(),
            domain: None,
            invite_code: Some(" code ".into()),
            email: Some(" Alice@Example.com ".into()),
            password: None,
            client_id: None,
            request_uri: None,
        };
        let values = form.values();
        assert_eq!(values.segment, "alice");
        assert_eq!(values.domain, "");
        assert_eq!(values.invite_code, "code");
        assert_eq!(values.email, "alice@example.com");
    }
}
