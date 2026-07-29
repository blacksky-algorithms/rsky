use crate::account_manager::AccountManager;
use crate::apis::com::atproto::identity::resolve_identity::inner_resolve_identity;
use crate::apis::ApiError;
use crate::SharedIdResolver;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::identity::{IdentityInfo, RefreshIdentityInput};

/// Request that the server re-resolve an identity (DID and handle). Bypasses
/// the DID cache and returns the freshly resolved identity.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.identity.refreshIdentity",
    format = "json",
    data = "<body>"
)]
pub async fn refresh_identity(
    body: Json<RefreshIdentityInput>,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<Json<IdentityInfo>, ApiError> {
    let RefreshIdentityInput { identifier } = body.into_inner();
    let info = inner_resolve_identity(identifier, true, id_resolver, &account_manager).await?;
    Ok(Json(info))
}
