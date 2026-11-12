use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::space_error;
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use rocket::form::FromForm;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{ListSpacesOutput, SpaceInfo};
use rsky_space::space_id::SpaceId;

/// `listSpaces` query. The filters' wire names are `spaceType` and `spaceDid`;
/// `type` and `did`, their names before the 2026-10-01 spaces alpha, are still
/// accepted. A field with more than one `name` matches any of them.
#[derive(FromForm)]
pub struct ListSpacesQuery {
    #[field(name = "spaceType")]
    #[field(name = "type")]
    pub space_type: Option<String>,
    #[field(name = "spaceDid")]
    #[field(name = "did")]
    pub did: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// The spaces the caller holds a repo in. `spaceType` narrows to one space type;
/// each entry carries the viewer's relationship to the space, as clients that
/// do not follow up with `getSpace` need.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.listSpaces?<query..>")]
pub async fn space_list_spaces(
    query: ListSpacesQuery,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<ListSpacesOutput>, ApiError> {
    let ListSpacesQuery {
        space_type,
        did: authority,
        limit,
        cursor,
    } = query;
    let did = auth
        .access
        .credentials
        .expect("credentials populated")
        .did
        .expect("did populated");
    let limit = limit.unwrap_or(50).clamp(1, 100) as usize;
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    let rows = reader
        .space
        .list_spaces(limit, cursor, space_type, authority)
        .await
        .map_err(space_error)?;
    let cursor = if rows.len() == limit {
        rows.last().map(|(uri, _, _)| uri.clone())
    } else {
        None
    };
    // Everything listed is a space the viewer holds a repo in, so `isMember`
    // is always true; `isOwner` distinguishes the ones the viewer anchors.
    // The authority is preferred from the row, with the URI as a fallback.
    let spaces = rows
        .into_iter()
        .map(|(uri, authority, created_at)| {
            let is_owner = authority == did
                || SpaceId::parse(&uri)
                    .map(|s| s.authority == did)
                    .unwrap_or(false);
            SpaceInfo {
                uri,
                is_owner,
                is_member: true,
                created_at,
            }
        })
        .collect();
    Ok(Json(ListSpacesOutput { cursor, spaces }))
}
