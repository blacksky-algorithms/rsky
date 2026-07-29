use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::local_space_def;
use crate::apis::com::atproto::space::{
    format_expiry, notify_expiry, parse_space_uri, space_error,
};
use crate::apis::ApiError;
use crate::space_auth::SpaceCredentialAuth;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{RegisterNotifyInput, RegisterNotifyOutput};

/// Register an endpoint for write notifications. With a `repo`, subscribes to
/// that repo on this repo host; without one, subscribes to the whole space on
/// this space host (spec §Write notifications).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.registerNotify",
    format = "json",
    data = "<body>"
)]
pub async fn space_register_notify(
    body: Json<RegisterNotifyInput>,
    auth: SpaceCredentialAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<RegisterNotifyOutput>, ApiError> {
    let RegisterNotifyInput {
        space,
        endpoint,
        repo,
    } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    if auth.space_uri != space_id.uri() {
        return Err(ApiError::InvalidToken);
    }
    if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
        return Err(ApiError::InvalidRequest(format!(
            "invalid endpoint: {endpoint}"
        )));
    }
    let expiry = notify_expiry();
    let expires_at = format_expiry(&expiry);
    match repo {
        Some(repo_did) => {
            let reader = actor_store
                .read(
                    repo_did.clone(),
                    blobstore_factory.blobstore(repo_did.clone()),
                )
                .await
                .map_err(|error| {
                    ApiError::BadRequest("RepoNotFound".to_string(), error.to_string())
                })?;
            reader
                .space
                .live_repo_state(&space_id.uri())
                .await
                .map_err(space_error)?;
            reader
                .space
                .register_repo_notify(&space_id.uri(), &endpoint, &expires_at)
                .await
                .map_err(space_error)?;
        }
        None => {
            let (_, space_store, _) =
                local_space_def(actor_store, blobstore_factory, &space_id).await?;
            space_store
                .register_host_notify(&space_id.uri(), &endpoint, &expires_at)
                .await
                .map_err(space_error)?;
        }
    }
    Ok(Json(RegisterNotifyOutput { expires_at: expiry }))
}
