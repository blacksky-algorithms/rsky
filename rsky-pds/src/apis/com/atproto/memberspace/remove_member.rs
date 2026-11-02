use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::memberspace::require_memberspace;
use crate::apis::com::atproto::space::{parse_space_uri, require_manage, space_def_error};
use crate::apis::ApiError;
use crate::apis::com::atproto::space::publish::unpublish_member;
use crate::config::ServerConfig;
use crate::auth_verifier::AccessSpace;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::memberspace::RemoveMemberInput;

/// Remove a member; they also leave the writer set (spec §memberspace).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.memberspace.removeMember",
    format = "json",
    data = "<body>"
)]
pub async fn memberspace_remove_member(
    body: Json<RemoveMemberInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    let RemoveMemberInput { space, did: member } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    require_manage(&credentials, &did, &space_id, ManageOp::Update)?;
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    let def = reader
        .space
        .live_space_def(&space_id.uri())
        .await
        .map_err(space_def_error)?;
    require_memberspace(&def)?;
    reader
        .space
        .remove_member(&space_id.uri(), &member)
        .await
        .map_err(space_def_error)?;
    reader
        .space
        .remove_writer(&space_id.uri(), &member)
        .await
        .map_err(space_def_error)?;
    unpublish_member(
        actor_store,
        blobstore_factory,
        server_config,
        &did,
        &space_id,
        &member,
    )
    .await?;
    if actor_store.exists(&member).await.unwrap_or(false) {
        if let Ok(member_reader) = actor_store
            .read(member.clone(), blobstore_factory.blobstore(member.clone()))
            .await
        {
            if let Err(error) = member_reader.space.remove_joined(&space_id.uri()).await {
                tracing::warn!(%error, "failed to unindex membership on the member's store");
            }
        }
    }
    Ok(())
}
