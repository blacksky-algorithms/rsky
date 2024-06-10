use crate::account_manager::helpers::account::ActorAccount;
use crate::account_manager::AccountManager;
use crate::common::env::env_list;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::SharedIdResolver;
use anyhow::{bail, Result};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::identity::ResolveHandleOutput;

async fn inner_resolve_handle(
    handle: String,
    id_resolver: &State<SharedIdResolver>,
) -> Result<ResolveHandleOutput> {
    // @TODO: Implement normalizeAndEnsureValidHandle()
    let mut did: Option<String> = None;
    let user: Option<ActorAccount> = AccountManager::get_account(&handle, None).await?;

    match user {
        Some(user) => did = Some(user.did),
        None => {
            let supported_handle = env_list("PDS_SERVICE_HANDLE_DOMAINS")
                .iter()
                .find(|host| handle.ends_with(host.as_str()) || handle == host[1..])
                .is_some();
            // this should be in our DB & we couldn't find it, so fail
            if supported_handle {
                bail!("unable to resolve handle");
            }
        }
    }

    // this is not someone on our server, but we help with resolving anyway
    /* if (!did && ctx.appViewAgent) {
      did = await tryResolveFromAppView(ctx.appViewAgent, handle)
    } */

    if did.is_none() {
        let mut lock = id_resolver.id_resolver.write().await;
        did = lock.handle.resolve(&handle).await?;
    }

    match did {
        None => bail!("unable to resolve handle"),
        Some(did) => Ok(ResolveHandleOutput { did }),
    }
}

#[rocket::get("/xrpc/com.atproto.identity.resolveHandle?<handle>")]
pub async fn resolve_handle(
    handle: String,
    id_resolver: &State<SharedIdResolver>,
) -> Result<Json<ResolveHandleOutput>, status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_resolve_handle(handle, id_resolver).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
