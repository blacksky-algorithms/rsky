//! Deactivating, reactivating and deleting an account from its pages.

use super::manage::{page_message, SOMETHING_WENT_WRONG};
use super::routes::{account_href, account_id, nav_for, page_session, redirect};
use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::server::activate_account::activate_account_for;
use crate::apis::com::atproto::server::deactivate_account::deactivate_account_for;
use crate::apis::com::atproto::server::delete_account::{
    delete_verified_account, verify_account_deletion,
};
use crate::apis::com::atproto::server::request_account_delete::request_account_delete_for;
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::lifecycle::{DeletionContext, LifecycleStore};
use crate::models::models::EmailTokenPurpose;
use crate::oauth::routes::rotate_session;
use crate::oauth::routes::{check_form_origin, OAuthRequestInfo};
use crate::oauth::{now_secs, DeviceSession, SharedOAuthProvider};
use crate::rate_limits::{Caller, RateLimits};
use crate::ui::pages::account::{
    DeactivatePage, DeletePage, DeleteStep, ReactivateAccountPage, Section,
};
use crate::ui::respond::{render_page, UiHtml};
use crate::ui::UiState;
use crate::SharedSequencer;
use hmac::{Hmac, Mac};
use rocket::form::Form;
use rocket::http::{CookieJar, Status};
use rocket::response::Redirect;
use rocket::{FromForm, Route, State};
use rsky_oauth::store::AccountInfo;
use sha2::{Digest, Sha256};

/// How long the final confirmation may wait after the code and password
/// were checked.
const DELETE_INTENT_MAX_AGE: u64 = 5 * 60;
const INVALID_PASSWORD: &str = "Invalid password";
const CONFIRM_AGAIN: &str = "Please confirm again";

pub fn routes() -> Vec<Route> {
    rocket::routes![
        deactivate_page_route,
        deactivate,
        reactivate_page_route,
        reactivate,
        delete_page_route,
        delete_request,
        delete_verify,
        delete_confirm,
    ]
}

fn manage_href(account: &AccountInfo) -> String {
    format!("{}/manage", account_href(&account_id(account)))
}

/// The attestation the final delete form carries: proof that this device
/// checked this code and the password, minutes ago at most.
pub fn delete_intent(key: &[u8; 32], did: &str, device_id: &str, code: &str, exp: u64) -> String {
    let code_hash = hex::encode(Sha256::digest(code.as_bytes()));
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(format!("{did}|{device_id}|{code_hash}|{exp}").as_bytes());
    format!("{}.{exp}", hex::encode(mac.finalize().into_bytes()))
}

/// Whether `intent` was minted by [`delete_intent`] for these inputs and
/// has not expired.
pub fn verify_delete_intent(
    key: &[u8; 32],
    did: &str,
    device_id: &str,
    code: &str,
    intent: &str,
    now: u64,
) -> bool {
    let Some((_, exp)) = intent.split_once('.') else {
        return false;
    };
    let Ok(exp) = exp.parse::<u64>() else {
        return false;
    };
    if exp < now {
        return false;
    }
    let expected = delete_intent(key, did, device_id, code, exp);
    expected.len() == intent.len()
        && expected
            .bytes()
            .zip(intent.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

fn deactivate_page(
    ui: &UiState,
    session: &DeviceSession,
    account: &AccountInfo,
    error: Option<String>,
) -> UiHtml {
    let base = manage_href(account);
    let page = DeactivatePage {
        shell: (*ui.shell).clone(),
        nav: nav_for(session, account, Section::Manage),
        error,
        submit_action: format!("{base}/deactivate"),
        cancel_href: base,
    };
    render_page(Status::Ok, &ui.shell, &page)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/manage/deactivate")]
pub async fn deactivate_page_route(
    id: &str,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now_secs()).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    if account.deactivated {
        return redirect(manage_href(&account));
    }
    Err(deactivate_page(ui, &session, &account, None))
}

#[derive(FromForm)]
pub struct PasswordFormData {
    pub csrf: String,
    pub password: String,
}

/// Deactivates with every credential revoked, as the reference's pages do,
/// after the password is confirmed.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/deactivate", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn deactivate(
    id: &str,
    form: Form<PasswordFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    sequencer: &State<SharedSequencer>,
) -> Result<Redirect, UiHtml> {
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now_secs()).await {
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
    match account_manager
        .verify_account_password(&account.did, &form.password)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            return Err(deactivate_page(
                ui,
                &session,
                &account,
                Some(INVALID_PASSWORD.to_string()),
            ))
        }
        Err(error) => {
            return Err(deactivate_page(
                ui,
                &session,
                &account,
                Some(page_message(&ApiError::from(error))),
            ))
        }
    }
    if let Err(error) =
        deactivate_account_for(&account.did, None, true, sequencer, account_manager).await
    {
        return Err(deactivate_page(
            ui,
            &session,
            &account,
            Some(page_message(&error)),
        ));
    }
    redirect(manage_href(&account))
}

fn reactivate_page(
    ui: &UiState,
    session: &DeviceSession,
    account: &AccountInfo,
    error: Option<String>,
) -> UiHtml {
    let base = manage_href(account);
    let page = ReactivateAccountPage {
        shell: (*ui.shell).clone(),
        nav: nav_for(session, account, Section::Manage),
        error,
        submit_action: format!("{base}/reactivate"),
        cancel_href: base,
    };
    render_page(Status::Ok, &ui.shell, &page)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/manage/reactivate")]
pub async fn reactivate_page_route(
    id: &str,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
) -> Result<Redirect, UiHtml> {
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now_secs()).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    if !account.deactivated {
        return redirect(manage_href(&account));
    }
    Err(reactivate_page(ui, &session, &account, None))
}

#[derive(FromForm)]
pub struct CsrfFormData {
    pub csrf: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/reactivate", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn reactivate(
    id: &str,
    form: Form<CsrfFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
) -> Result<Redirect, UiHtml> {
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now_secs()).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    if !account.deactivated {
        return redirect(manage_href(&account));
    }
    check_form_origin(
        &ui.shell,
        &info,
        &session,
        &form.csrf,
        manage_href(&account),
    )?;
    if let Err(error) = activate_account_for(
        account.did.clone(),
        sequencer,
        blobstore_factory,
        actor_store,
        account_manager,
    )
    .await
    {
        tracing::warn!(%error, did = %account.did, "reactivation from the account pages failed");
        return Err(reactivate_page(
            ui,
            &session,
            &account,
            Some(SOMETHING_WENT_WRONG.to_string()),
        ));
    }
    redirect(format!("{}?notice=reactivated", manage_href(&account)))
}

#[allow(clippy::too_many_arguments)]
fn delete_page(
    ui: &UiState,
    session: &DeviceSession,
    account: &AccountInfo,
    email: Option<String>,
    step: DeleteStep,
    code: String,
    intent: String,
    error: Option<String>,
) -> UiHtml {
    let base = manage_href(account);
    let page = DeletePage {
        shell: (*ui.shell).clone(),
        nav: nav_for(session, account, Section::Manage),
        step,
        email,
        error,
        code,
        intent,
        request_action: format!("{base}/delete/request"),
        verify_action: format!("{base}/delete/verify"),
        confirm_action: format!("{base}/delete/confirm"),
        cancel_href: base,
    };
    render_page(Status::Ok, &ui.shell, &page)
}

async fn account_email(account_manager: &AccountManager, did: &str) -> Option<String> {
    account_manager
        .get_account(
            did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: None,
            }),
        )
        .await
        .ok()
        .flatten()
        .and_then(|account| account.email)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/account/u/<id>/manage/delete")]
pub async fn delete_page_route(
    id: &str,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
) -> Result<Redirect, UiHtml> {
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now_secs()).await {
            Ok(found) => found,
            Err(answer) => return answer,
        };
    let email = account_email(account_manager, &account.did).await;
    Err(delete_page(
        ui,
        &session,
        &account,
        email,
        DeleteStep::Request,
        String::new(),
        String::new(),
        None,
    ))
}

/// Step one: the code is mailed. Nothing is kept server-side beyond the
/// token row itself.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/delete/request", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn delete_request(
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
    let (session, account) =
        match page_session(ui, shared, jar, &info, id, Section::Manage, now_secs()).await {
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
    let email = account_email(account_manager, &account.did).await;
    let result = match limits
        .consume_all(
            &crate::rate_limits::REQUEST_ACCOUNT_DELETE,
            &account.did,
            1,
            caller.bypass,
        )
        .await
    {
        Ok(()) => request_account_delete_for(&account.did, account_manager).await,
        Err(error) => Err(error),
    };
    let (step, error) = match result {
        Ok(()) => (DeleteStep::Confirm, None),
        Err(error) => (DeleteStep::Request, Some(page_message(&error))),
    };
    Err(delete_page(
        ui,
        &session,
        &account,
        email,
        step,
        String::new(),
        String::new(),
        error,
    ))
}

#[derive(FromForm)]
pub struct DeleteVerifyFormData {
    pub csrf: String,
    pub code: String,
    pub password: String,
}

/// Step two: the code and the password are checked, and the last page gets
/// an attestation of that check instead of the password.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/delete/verify", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn delete_verify(
    id: &str,
    form: Form<DeleteVerifyFormData>,
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
    let email = account_email(account_manager, &account.did).await;
    let code = form.code.trim().to_string();
    let checked = match limits
        .consume_all(
            &crate::rate_limits::DELETE_ACCOUNT,
            &caller.ip,
            1,
            caller.bypass,
        )
        .await
    {
        Ok(()) => {
            verify_account_deletion(&account.did, &form.password, &code, account_manager).await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = checked {
        return Err(delete_page(
            ui,
            &session,
            &account,
            email,
            DeleteStep::Confirm,
            String::new(),
            String::new(),
            Some(page_message(&error)),
        ));
    }
    let intent = delete_intent(
        &ui.intent_key,
        &account.did,
        &session.device_id,
        &code,
        now + DELETE_INTENT_MAX_AGE,
    );
    Err(delete_page(
        ui,
        &session,
        &account,
        email,
        DeleteStep::FinalConfirm,
        code,
        intent,
        None,
    ))
}

#[derive(FromForm)]
pub struct DeleteConfirmFormData {
    pub csrf: String,
    pub code: String,
    pub delete_intent: String,
}

/// Step three: with a live attestation and the still-valid code, the
/// account goes, and so does every session of it on every device.
#[tracing::instrument(skip_all)]
#[rocket::post("/account/u/<id>/manage/delete/confirm", data = "<form>")]
#[allow(clippy::too_many_arguments)]
pub async fn delete_confirm(
    id: &str,
    form: Form<DeleteConfirmFormData>,
    jar: &CookieJar<'_>,
    info: OAuthRequestInfo,
    shared: &State<SharedOAuthProvider>,
    ui: &State<UiState>,
    account_manager: &State<AccountManager>,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    lifecycle_store: &State<LifecycleStore>,
    cfg: &State<ServerConfig>,
) -> Result<Redirect, UiHtml> {
    let now = now_secs();
    let (mut session, account) =
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
    let email = account_email(account_manager, &account.did).await;
    let code = form.code.trim().to_string();
    let confirm_again = |error: &str| {
        delete_page(
            ui,
            &session,
            &account,
            email.clone(),
            DeleteStep::Confirm,
            String::new(),
            String::new(),
            Some(error.to_string()),
        )
    };
    if !verify_delete_intent(
        &ui.intent_key,
        &account.did,
        &session.device_id,
        &code,
        &form.delete_intent,
        now,
    ) {
        return Err(confirm_again(CONFIRM_AGAIN));
    }
    if let Err(error) = account_manager
        .assert_valid_email_token(&account.did, EmailTokenPurpose::DeleteAccount, &code)
        .await
    {
        return Err(confirm_again(&page_message(&ApiError::from(error))));
    }
    let blobstore =
        (!cfg.service.coexistence).then(|| blobstore_factory.blobstore(account.did.clone()));
    let ctx = DeletionContext {
        lifecycle: lifecycle_store,
        account_manager,
        sequencer,
        actor_store,
        blobstore,
    };
    if let Err(error) = delete_verified_account(&account.did, &ctx).await {
        return Err(confirm_again(&page_message(&error)));
    }
    let store = shared.provider.store();
    if let Ok(devices) = store.list_account_devices(&account.did).await {
        for linked in devices {
            if let Err(error) = store
                .remove_device_account(&linked.device_id, &account.did)
                .await
            {
                tracing::warn!(%error, "device session outlived the deleted account");
            }
        }
    }
    if let Err(error) = rotate_session(shared, jar, &mut session).await {
        tracing::warn!(%error, "device secret not rotated after account deletion");
    }
    redirect("/account".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_intents_bind_did_device_code_and_time() {
        let key = [7u8; 32];
        let intent = delete_intent(&key, "did:plc:a", "dev-1", "CODE-1", 1_000);
        assert!(intent.ends_with(".1000"));
        assert!(verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-1",
            "CODE-1",
            &intent,
            999
        ));
        assert!(verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-1",
            "CODE-1",
            &intent,
            1_000
        ));
        assert!(!verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-1",
            "CODE-1",
            &intent,
            1_001
        ));
        assert!(!verify_delete_intent(
            &key,
            "did:plc:b",
            "dev-1",
            "CODE-1",
            &intent,
            999
        ));
        assert!(!verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-2",
            "CODE-1",
            &intent,
            999
        ));
        assert!(!verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-1",
            "CODE-2",
            &intent,
            999
        ));
        assert!(!verify_delete_intent(
            &[8u8; 32],
            "did:plc:a",
            "dev-1",
            "CODE-1",
            &intent,
            999
        ));
        assert!(!verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-1",
            "CODE-1",
            "garbage",
            999
        ));
        assert!(!verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-1",
            "CODE-1",
            "abc.notanumber",
            999
        ));
        let forged = format!("{}.1000", "0".repeat(64));
        assert!(!verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-1",
            "CODE-1",
            &forged,
            999
        ));
        let short = format!("{}.1000", "0".repeat(10));
        assert!(!verify_delete_intent(
            &key,
            "did:plc:a",
            "dev-1",
            "CODE-1",
            &short,
            999
        ));
    }
}
