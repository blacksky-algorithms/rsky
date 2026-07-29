use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::simplespace::{require_manage, space_error};
use crate::apis::com::atproto::space::parse_space_uri;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::simplespace::AddMemberInput;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.simplespace.addMember",
    format = "json",
    data = "<body>"
)]
pub async fn simplespace_add_member(
    body: Json<AddMemberInput>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<(), ApiError> {
    let AddMemberInput { space, did: member } = body.into_inner();
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
    reader
        .space
        .live_space_def(&space_id.uri())
        .await
        .map_err(space_error)?;
    reader
        .space
        .add_member(&space_id.uri(), &member)
        .await
        .map_err(space_error)?;
    Ok(())
}
