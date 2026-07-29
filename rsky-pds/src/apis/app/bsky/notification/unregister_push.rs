use crate::apis::app::bsky::notification::register_push::get_endpoint;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use crate::{context, SharedIdResolver, APP_USER_AGENT};
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::notification::RegisterPushInput;
use rsky_repo::types::Ids;

pub async fn inner_unregister_push(
    body: Json<RegisterPushInput>,
    auth: AccessStandard,
    cfg: &State<ServerConfig>,
    app_view_url: String,
    id_resolver: &State<SharedIdResolver>,
) -> Result<()> {
    let input = body.into_inner();
    let did: String = match auth.access.credentials {
        None => "".to_string(),
        Some(credentials) => credentials.did.unwrap_or("".to_string()),
    };
    let nsid = Ids::AppBskyNotificationUnregisterPush.as_str().to_string();
    let auth_headers = context::service_auth_headers(&did, &input.service_did, &nsid).await?;

    let url = if let Some(ref bsky_app_view) = cfg.bsky_app_view {
        if bsky_app_view.did == input.service_did {
            app_view_url
        } else {
            get_endpoint(id_resolver, input.service_did.clone()).await?
        }
    } else {
        get_endpoint(id_resolver, input.service_did.clone()).await?
    };

    let client = reqwest::ClientBuilder::new()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_millis(1000))
        .default_headers(auth_headers)
        .build()?;
    let res = client
        .post(format!("{url}/xrpc/{nsid}"))
        .json(&input)
        .send()
        .await?;
    if !res.status().is_success() {
        bail!("unable to unregister push notifications: {}", res.status());
    }
    Ok(())
}

/// The inverse of registerPush - inform a specified service that push notifications
/// should no longer be sent to the given token for the requesting account. Requires auth.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/app.bsky.notification.unregisterPush",
    format = "json",
    data = "<body>"
)]
pub async fn unregister_push(
    body: Json<RegisterPushInput>,
    auth: AccessStandard,
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
) -> Result<(), ApiError> {
    if !["ios", "android", "web"].contains(&body.platform.as_str()) {
        return Err(ApiError::InvalidRequest("invalid platform".to_string()));
    }
    match &cfg.bsky_app_view {
        None => Err(ApiError::RuntimeError),
        Some(bsky_app_view) => {
            match inner_unregister_push(body, auth, cfg, bsky_app_view.url.clone(), id_resolver)
                .await
            {
                Ok(_) => Ok(()),
                Err(error) => {
                    tracing::error!("@LOG: ERROR: {error}");
                    Err(ApiError::RuntimeError)
                }
            }
        }
    }
}
