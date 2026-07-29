use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::SpaceWrite;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{apply_space_writes, commit_meta, parse_space_uri};
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::config::ServerConfig;
use crate::space_auth::session_permits;
use crate::space_scope::{SpaceAction, SpaceRequest};
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{DeleteRecordInput, DeleteRecordOutput};

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.deleteRecord",
    format = "json",
    data = "<body>"
)]
pub async fn space_delete_record(
    body: Json<DeleteRecordInput>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<Json<DeleteRecordOutput>, ApiError> {
    let DeleteRecordInput {
        space,
        collection,
        rkey,
        swap_record,
    } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    if !session_permits(
        &credentials,
        &did,
        &space_id,
        &SpaceRequest::Write {
            action: SpaceAction::Delete,
            collection: collection.clone(),
        },
    ) {
        return Err(ApiError::AuthRequiredError(
            "session does not cover this space".to_string(),
        ));
    }
    let commit = apply_space_writes(
        actor_store,
        blobstore_factory,
        server_config,
        &did,
        &space_id,
        vec![SpaceWrite::Delete {
            collection,
            rkey,
            swap_cid: swap_record,
        }],
    )
    .await?;
    Ok(Json(DeleteRecordOutput {
        commit: Some(commit_meta(&commit)),
    }))
}
