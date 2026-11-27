use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::memberspace::require_memberspace;
use crate::apis::com::atproto::space::{parse_space_uri, require_read_self, space_def_error};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::memberspace::{ListMembersOutput, Member};

/// List a memberspace's members and their `read`/`write` access. A `read_self`
/// grant suffices — this is a query, not a management procedure.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.memberspace.listMembers?<space>&<limit>&<cursor>")]
pub async fn memberspace_list_members(
    space: String,
    limit: Option<i64>,
    cursor: Option<String>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<ListMembersOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    require_read_self(&credentials, &did, &space_id)?;
    let limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
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
    let members = reader
        .space
        .list_members(&space_id.uri(), limit, cursor)
        .await
        .map_err(space_def_error)?;
    let cursor = if members.len() == limit {
        members.last().map(|row| row.did.clone())
    } else {
        None
    };
    Ok(Json(ListMembersOutput {
        cursor,
        members: members
            .into_iter()
            .map(|row| Member {
                did: row.did,
                read: row.read,
                write: row.write,
                member_rev: (!row.member_rev.is_empty()).then_some(row.member_rev),
                added_at: (!row.added_at.is_empty()).then_some(row.added_at),
            })
            .collect(),
    }))
}
