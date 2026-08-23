use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::local_space_def;
use crate::apis::com::atproto::space::register_notify::resolve_subscriber;
use crate::apis::com::atproto::space::{parse_space_uri, space_error};
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::space_auth::SpaceCredentialAuth;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::UnregisterNotifyInput;

/// Withdraw a write-notification registration, mirroring `registerNotify`:
/// with a `repo` it withdraws that repo's registration on this repo host,
/// without one the whole-space registration on this space host.
///
/// Idempotent. A caller retrying after a timeout, or withdrawing a
/// registration that already lapsed, is asking for the same end state as one
/// that removed a live row.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.unregisterNotify",
    format = "json",
    data = "<body>"
)]
pub async fn space_unregister_notify(
    body: Json<UnregisterNotifyInput>,
    auth: SpaceCredentialAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    let UnregisterNotifyInput {
        space,
        service,
        endpoint,
        repo,
    } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    if auth.space_uri != space_id.uri() {
        return Err(ApiError::InvalidToken);
    }
    // Registrations are keyed by endpoint, so a `service` is resolved the same
    // way `registerNotify` resolved it to find the row it wrote.
    let endpoint = match (&service, &endpoint) {
        (Some(service), _) => resolve_subscriber(&server_config.identity.plc_url, service).await?,
        (None, Some(endpoint)) => endpoint.clone(),
        (None, None) => {
            return Err(ApiError::InvalidRequest(
                "unregisterNotify requires a service or an endpoint".to_string(),
            ))
        }
    };
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
                .unregister_repo_notify(&space_id.uri(), &endpoint)
                .await
                .map_err(space_error)?;
        }
        None => {
            let (_, space_store, _) =
                local_space_def(actor_store, blobstore_factory, &space_id).await?;
            space_store
                .unregister_host_notify(&space_id.uri(), &endpoint)
                .await
                .map_err(space_error)?;
        }
    }
    Ok(())
}
