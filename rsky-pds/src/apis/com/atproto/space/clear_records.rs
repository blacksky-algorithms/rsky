use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::local_space_def;
use crate::apis::com::atproto::space::publish::MANAGED_COLLECTIONS;
use crate::apis::com::atproto::space::{
    notify_after_write, parse_space_uri, require_repo_matches_subject, space_error,
};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::config::ServerConfig;
use crate::space_auth::session_permits;
use crate::space_scope::{SpaceAction, SpaceRequest};
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::ClearRecordsInput;

/// `com.atproto.space.clearRecords`: delete every record in the
/// authenticated account's space repo. The repo advances to a fresh rev with
/// the remaining records' digest and its oplog is emptied; in the space
/// authority's own repo the published policy/member records are kept (spec
/// §Repo lifecycle).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.clearRecords",
    format = "json",
    data = "<body>"
)]
pub async fn space_clear_records(
    body: Json<ClearRecordsInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ClearRecordsInput { space, repo } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    require_repo_matches_subject(&repo, &did)?;
    // Clearing is a whole-repo delete: a session needs a covering delete grant.
    if !session_permits(
        &credentials,
        &did,
        &space_id,
        &SpaceRequest::Write {
            action: SpaceAction::Delete,
            collection: "com.atproto.space.clearRecords".to_string(),
        },
    ) {
        return Err(ApiError::AuthRequiredError(
            "session does not cover clearing this repo".to_string(),
        ));
    }
    if space_id.authority == did {
        // An authority's space must already exist (spec §Repo lifecycle).
        local_space_def(actor_store, blobstore_factory, &space_id).await?;
    }
    let transactor = actor_store
        .transact(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    let keep: &[&str] = if did == space_id.authority {
        &MANAGED_COLLECTIONS
    } else {
        &[]
    };
    let commit = transactor
        .space
        .clear_repo(&space_id, keep)
        .await
        .map_err(space_error)?;
    notify_after_write(
        actor_store,
        blobstore_factory,
        server_config,
        transactor.keypair,
        transactor.space.clone(),
        &did,
        &space_id,
        &commit,
    )
    .await;
    Ok(Json(serde_json::json!({})))
}
