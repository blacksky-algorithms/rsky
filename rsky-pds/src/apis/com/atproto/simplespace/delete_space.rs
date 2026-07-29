use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::simplespace::{actor_keypair, require_manage, space_error};
use crate::apis::com::atproto::space::{
    parse_space_uri, queue_space_deleted_notifications, resolve_space_host_endpoint,
};
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::config::ServerConfig;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::simplespace::DeleteSpaceInput;

/// Delete a space: stop minting credentials, flag local repos, and notify
/// registered syncers and writers' hosts best-effort (spec §Space deletion).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.simplespace.deleteSpace",
    format = "json",
    data = "<body>"
)]
pub async fn simplespace_delete_space(
    body: Json<DeleteSpaceInput>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    let DeleteSpaceInput { space } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    require_manage(&credentials, &did, &space_id, ManageOp::Delete)?;
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    reader
        .space
        .live_space_def(&space_id.uri())
        .await
        .map_err(space_error)?;
    // Stop answering for the space and delete the authority's own repo.
    reader
        .space
        .flag_space_def_deleted(&space_id.uri())
        .await
        .map_err(space_error)?;
    reader
        .space
        .flag_repo_deleted(&space_id.uri())
        .await
        .map_err(space_error)?;

    // Collect notification targets: registered syncers plus the repo hosts of
    // every known writer. Local writers are flagged directly.
    let now = rsky_common::now();
    let mut endpoints = reader
        .space
        .host_notify_endpoints(&space_id.uri(), &now)
        .await
        .map_err(space_error)?;
    let writers = reader
        .space
        .list_writers(&space_id.uri(), usize::MAX >> 1, None)
        .await
        .map_err(space_error)?;
    let mut remote_writers: Vec<String> = Vec::new();
    for writer in writers {
        if writer.did == did {
            continue;
        }
        if actor_store.exists(&writer.did).await.unwrap_or(false) {
            if let Ok(writer_reader) = actor_store
                .read(
                    writer.did.clone(),
                    blobstore_factory.blobstore(writer.did.clone()),
                )
                .await
            {
                if let Err(error) = writer_reader.space.flag_repo_deleted(&space_id.uri()).await {
                    tracing::warn!(%error, did = %writer.did, "failed to flag writer repo");
                }
            }
        } else {
            remote_writers.push(writer.did);
        }
    }
    let keypair = actor_keypair(actor_store, &did).await?;
    let plc_url = server_config.identity.plc_url.clone();
    if !remote_writers.is_empty() {
        // Resolve remote writers' PDS endpoints in the background, then fold
        // them into the notification fan-out.
        let authority = space_id.authority.clone();
        let space_uri = space_id.uri();
        actor_store.background_queue.add(async move {
            let mut writer_endpoints = Vec::new();
            for writer in remote_writers {
                match resolve_space_host_endpoint(&plc_url, &writer).await {
                    Ok(endpoint) => writer_endpoints.push(endpoint),
                    Err(error) => {
                        tracing::debug!(%error, %writer, "could not resolve writer host")
                    }
                }
            }
            crate::apis::com::atproto::space::deliver_notifications(
                &keypair,
                &authority,
                &authority,
                crate::space_auth::NOTIFY_SPACE_DELETED_LXM,
                &writer_endpoints,
                &serde_json::json!({ "space": space_uri }),
            )
            .await;
            Ok(())
        });
    }
    endpoints.sort();
    endpoints.dedup();
    queue_space_deleted_notifications(
        actor_store,
        keypair,
        space_id.authority.clone(),
        space_id.uri(),
        endpoints,
    );
    Ok(())
}
