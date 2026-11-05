use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::memberspace::list_members::memberspace_list_members;
use crate::apis::com::atproto::simplespace::adapters::new_member_to_old;
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use rocket::serde::json::Json;
use rocket::State;
#[allow(deprecated)]
use rsky_lexicon::com::atproto::simplespace::ListMembersOutput;

/// DEPRECATED: List members using old simplespace API.
///
/// Note: Response strips read/write access info that new API returns.
///
/// Supported indefinitely (legacy API)
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.simplespace.listMembers?<space>&<limit>&<cursor>")]
pub async fn simplespace_list_members(
    space: String,
    limit: Option<i64>,
    cursor: Option<String>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<ListMembersOutput>, ApiError> {
    // Legacy API - supported indefinitely
    // Translates new member format to old format (preserves member_rev/added_at)

    // Forward to new implementation
    let result =
        memberspace_list_members(space, limit, cursor, auth, actor_store, blobstore_factory)
            .await?;

    let new_output = result.into_inner();

    // Translate response: preserve member_rev/added_at, strip read/write fields
    let old_members = new_output.members.iter().map(new_member_to_old).collect();

    Ok(Json(ListMembersOutput {
        members: old_members,
        cursor: new_output.cursor,
    }))
}
