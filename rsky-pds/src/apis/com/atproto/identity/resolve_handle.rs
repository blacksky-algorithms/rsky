use crate::account_manager::helpers::account::ActorAccount;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::{SharedIdResolver, APP_USER_AGENT};
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::env::{env_list, env_str};
use rsky_lexicon::com::atproto::identity::ResolveHandleOutput;

async fn try_resolve_from_app_view(handle: &String) -> Result<Option<String>> {
    match env_str("PDS_BSKY_APP_VIEW_URL") {
        None => Ok(None),
        Some(bsky_app_view_url) => {
            let client = reqwest::Client::builder()
                .user_agent(APP_USER_AGENT)
                .build()?;
            let params = Some(vec![("handle", handle)]);
            let res = client
                .get(format!(
                    "{bsky_app_view_url}/xrpc/com.atproto.identity.resolveHandle"
                ))
                .header("Connection", "Keep-Alive")
                .header("Keep-Alive", "timeout=5, max=1000")
                .query(&params)
                .send()
                .await;
            match res {
                Err(_) => Ok(None),
                Ok(res) => match res.json::<ResolveHandleOutput>().await {
                    Err(_) => Ok(None),
                    Ok(data) => Ok(Some(data.did)),
                },
            }
        }
    }
}

async fn inner_resolve_handle(
    handle: String,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<ResolveHandleOutput> {
    // @TODO: Implement normalizeAndEnsureValidHandle()
    let mut did: Option<String> = None;
    let user: Option<ActorAccount> = account_manager.get_account(&handle, None).await?;

    match user {
        Some(user) => did = Some(user.did),
        None => {
            let supported_handle = env_list("PDS_SERVICE_HANDLE_DOMAINS")
                .iter()
                .any(|host| handle.ends_with(host.as_str()) || handle == host[1..]);
            // this should be in our DB & we couldn't find it, so fail
            if supported_handle {
                bail!("unable to resolve handle");
            }
        }
    }

    // this is not someone on our server, but we help with resolving anyway
    // @TODO: Weird error about Tokio received when this fails that leads to panic
    if did.is_none() && env_str("PDS_BSKY_APP_VIEW_URL").is_some() {
        did = try_resolve_from_app_view(&handle).await?;
    }

    if did.is_none() {
        let mut lock = id_resolver.id_resolver.write().await;
        did = lock.handle.resolve(&handle).await?;
    }

    match did {
        None => bail!("unable to resolve handle"),
        Some(did) => Ok(ResolveHandleOutput { did }),
    }
}

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.identity.resolveHandle?<handle>")]
pub async fn resolve_handle(
    handle: String,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<Json<ResolveHandleOutput>, ApiError> {
    match inner_resolve_handle(handle, id_resolver, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
