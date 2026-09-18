use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::lifecycle::{self, DeletionContext, LifecycleStore};
use crate::models::models::EmailTokenPurpose;
use crate::rate_limits::{Caller, RateLimits};
use crate::SharedSequencer;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::DeleteAccountInput;

/// Passwords longer than this were never accepted, so a longer one cannot
/// be a legitimate credential.
const OLD_PASSWORD_MAX_LENGTH: usize = 512;

/// Checks the password and the mailed token an account deletion needs,
/// without consuming the token.
pub(crate) async fn verify_account_deletion(
    did: &str,
    password: &str,
    token: &str,
    account_manager: &AccountManager,
) -> Result<(), ApiError> {
    if password.len() > OLD_PASSWORD_MAX_LENGTH {
        return Err(ApiError::AuthRequiredError(
            "Password too long. Consider resetting your password.".to_string(),
        ));
    }
    let account = account_manager
        .get_account(
            did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    if account.is_none() {
        return Err(ApiError::InvalidRequest("account not found".to_string()));
    }
    if !account_manager
        .verify_account_password(did, &password.to_string())
        .await?
    {
        return Err(ApiError::AuthRequiredError(
            "Invalid did or password".to_string(),
        ));
    }
    account_manager
        .assert_valid_email_token(did, EmailTokenPurpose::DeleteAccount, token)
        .await?;
    Ok(())
}

/// Deletes the account in the reference PDS's order: account rows first,
/// then the deletion event, then the actor store. The caller has verified
/// the credentials.
pub(crate) async fn delete_verified_account(
    did: &str,
    ctx: &DeletionContext<'_>,
) -> Result<(), ApiError> {
    lifecycle::delete_account(ctx, did, None).await?;
    Ok(())
}

/// Deletes the caller's account, authenticated by the account password and
/// the token mailed by `requestAccountDelete`.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.deleteAccount",
    format = "json",
    data = "<body>"
)]
#[allow(clippy::too_many_arguments)]
pub async fn delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    lifecycle_store: &State<LifecycleStore>,
    cfg: &State<ServerConfig>,
    account_manager: AccountManager,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<(), ApiError> {
    limits
        .consume_all(
            &crate::rate_limits::DELETE_ACCOUNT,
            &caller.ip,
            1,
            caller.bypass,
        )
        .await?;
    let DeleteAccountInput {
        did,
        password,
        token,
    } = body.into_inner();
    verify_account_deletion(&did, &password, &token, &account_manager).await?;

    let blobstore = (!cfg.service.coexistence).then(|| blobstore_factory.blobstore(did.clone()));
    delete_verified_account(
        &did,
        &DeletionContext {
            lifecycle: lifecycle_store,
            account_manager: &account_manager,
            sequencer,
            actor_store,
            blobstore,
        },
    )
    .await
}
