use crate::account_manager::helpers::account::{
    format_account_status, ActorAccount, AvailabilityFlags, FormattedAccountStatus,
};
use crate::account_manager::helpers::password::AppPassDescript;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::did_doc_for_session;
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::rate_limits::{Caller, RateLimits};
use crate::SharedIdResolver;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::{CreateSessionInput, CreateSessionOutput};
use rsky_syntax::handle::INVALID_HANDLE;
use std::time::{Duration, Instant};

/// Passwords longer than this were never accepted, so a longer one cannot
/// be a legitimate credential.
const OLD_PASSWORD_MAX_LENGTH: usize = 512;

/// A login takes at least this long, so a rejected password cannot be told
/// apart from an accepted one by timing.
const LOGIN_MIN_DURATION: Duration = Duration::from_millis(350);

struct Login {
    user: ActorAccount,
    app_password: Option<AppPassDescript>,
    is_soft_deleted: bool,
}

async fn login(
    identifier: &str,
    password: &str,
    account_manager: &AccountManager,
) -> Result<Login, ApiError> {
    let started = Instant::now();
    let result = login_inner(identifier, password, account_manager).await;
    if let Some(remaining) = LOGIN_MIN_DURATION.checked_sub(started.elapsed()) {
        tokio::time::sleep(remaining).await;
    }
    result
}

async fn login_inner(
    identifier: &str,
    password: &str,
    account_manager: &AccountManager,
) -> Result<Login, ApiError> {
    let identifier = identifier.to_lowercase();
    let flags = Some(AvailabilityFlags {
        include_deactivated: Some(true),
        include_taken_down: Some(true),
    });
    let user = if identifier.contains('@') {
        account_manager
            .get_account_by_email(&identifier, flags)
            .await
    } else {
        account_manager.get_account(&identifier, flags).await
    };
    let Some(user) = user? else {
        return Err(ApiError::InvalidLogin);
    };
    let is_soft_deleted = user.takedown_ref.is_some();
    let valid_account_pass = account_manager
        .verify_account_password(&user.did, &password.to_owned())
        .await?;
    let mut app_password = None;
    if !valid_account_pass {
        // takendown/suspended accounts cannot login with app password
        if is_soft_deleted {
            return Err(ApiError::InvalidLogin);
        }
        app_password = account_manager
            .verify_app_password(&user.did, password)
            .await?;
        if app_password.is_none() {
            return Err(ApiError::InvalidLogin);
        }
    }
    Ok(Login {
        user,
        app_password,
        is_soft_deleted,
    })
}

#[tracing::instrument(skip_all)]
async fn inner_create_session(
    body: Json<CreateSessionInput>,
    cfg: &ServerConfig,
    id_resolver: &SharedIdResolver,
    account_manager: AccountManager,
) -> Result<CreateSessionOutput, ApiError> {
    let CreateSessionInput {
        password,
        identifier,
        allow_takendown,
    } = body.into_inner();
    if password.len() > OLD_PASSWORD_MAX_LENGTH {
        return Err(ApiError::AuthRequiredError(
            "Password too long. Consider resetting your password.".to_string(),
        ));
    }

    let Login {
        user,
        app_password,
        is_soft_deleted,
    } = login(&identifier, &password, &account_manager).await?;

    if !allow_takendown.unwrap_or(false) && is_soft_deleted {
        return Err(ApiError::AccountTakendown);
    }
    let (access_jwt, refresh_jwt) = account_manager
        .create_session(user.did.clone(), app_password, is_soft_deleted)
        .await?;
    let FormattedAccountStatus { active, status } = format_account_status(Some(user.clone()));
    let did_doc = did_doc_for_session(
        cfg.identity.enable_did_doc_with_session,
        id_resolver,
        &user.did,
    )
    .await;
    Ok(CreateSessionOutput {
        did: user.did,
        did_doc,
        handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
        email: user.email,
        email_confirmed: Some(user.email_confirmed_at.is_some()),
        access_jwt,
        refresh_jwt,
        active: Some(active),
        status: status.map(|status| status.as_str().to_owned()),
    })
}

#[rocket::post(
    "/xrpc/com.atproto.server.createSession",
    format = "json",
    data = "<body>"
)]
pub async fn create_session(
    body: Json<CreateSessionInput>,
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Json<CreateSessionOutput>, ApiError> {
    limits.consume_all(
        &crate::rate_limits::CREATE_SESSION,
        &format!("{}-{}", body.identifier, caller.ip),
        1,
        caller.bypass,
    )?;
    match inner_create_session(body, cfg, id_resolver, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => Err(error),
    }
}
