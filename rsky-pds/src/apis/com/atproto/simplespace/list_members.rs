use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::simplespace::{require_manage, space_error};
use crate::apis::com::atproto::space::parse_space_uri;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::simplespace::{ListMembersOutput, Member};

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.simplespace.listMembers?<space>&<limit>&<cursor>")]
pub async fn simplespace_list_members(
    space: String,
    limit: Option<i64>,
    cursor: Option<String>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<ListMembersOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    require_manage(&credentials, &did, &space_id, ManageOp::Update)?;
    let limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    reader
        .space
        .live_space_def(&space_id.uri())
        .await
        .map_err(space_error)?;
    let members = reader
        .space
        .list_members(&space_id.uri(), limit, cursor)
        .await
        .map_err(space_error)?;
    let cursor = if members.len() == limit {
        members.last().cloned()
    } else {
        None
    };
    Ok(Json(ListMembersOutput {
        cursor,
        members: members.into_iter().map(|did| Member { did }).collect(),
    }))
}
