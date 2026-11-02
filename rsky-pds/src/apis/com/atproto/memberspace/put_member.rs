use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::memberspace::require_memberspace;
use crate::apis::com::atproto::space::{parse_space_uri, require_manage, space_def_error};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::memberspace::PutMemberInput;

/// Add a member or replace their `read`/`write` access together (spec
/// §memberspace). Revoking write drops the member from the writer set.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.memberspace.putMember",
    format = "json",
    data = "<body>"
)]
pub async fn memberspace_put_member(
    body: Json<PutMemberInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<(), ApiError> {
    let PutMemberInput {
        space,
        did: member,
        read,
        write,
    } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    require_manage(&credentials, &did, &space_id, ManageOp::Update)?;
    if !member.starts_with("did:") {
        return Err(ApiError::InvalidRequest(format!("invalid did: {member}")));
    }
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
        .put_member(&space_id.uri(), &member, read, write)
        .await
        .map_err(space_def_error)?;
    if !write {
        reader
            .space
            .remove_writer(&space_id.uri(), &member)
            .await
            .map_err(space_def_error)?;
    }
    // A membership must be discoverable through the member's own listSpaces
    // before their first write creates a repo row. Only a local member's
    // store can be written from here; a remote member's PDS learns through
    // its own flows.
    if actor_store.exists(&member).await.unwrap_or(false) {
        if let Ok(member_reader) = actor_store
            .read(member.clone(), blobstore_factory.blobstore(member.clone()))
            .await
        {
            if let Err(error) = member_reader.space.record_joined(&space_id).await {
                tracing::warn!(%error, "failed to index membership on the member's store");
            }
        }
    }
    Ok(())
}
