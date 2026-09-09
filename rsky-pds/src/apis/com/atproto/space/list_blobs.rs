use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{open_local_repo, parse_space_uri, space_error};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::ListBlobsOutput;

const DEFAULT_LIMIT: i64 = 500;
const MAX_LIMIT: i64 = 1000;

/// The CIDs of blobs referenced by records in a repo's space, for a syncer
/// mirroring the repo's blobs (spec §Blobs).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.listBlobs?<space>&<repo>&<limit>&<cursor>")]
#[allow(clippy::too_many_arguments)]
pub async fn space_list_blobs(
    space: String,
    repo: String,
    limit: Option<i64>,
    cursor: Option<String>,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<Json<ListBlobsOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &repo,
        &SpaceRequest::ReadSelf { collection: None },
    )?;
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &repo,
        false,
    )
    .await?;
    // A repo with no rows has no blobs to list, which is an empty page rather
    // than a missing space.
    reader
        .space
        .readable_repo_state(&space_id.uri())
        .await
        .map_err(space_error)?;
    let page = reader
        .space
        .list_space_blobs(&space_id.uri(), cursor, limit as usize)
        .await
        .map_err(space_error)?;
    // The cursor follows the rows read, not the rows returned, so filtering
    // cannot make the next page skip anything.
    let cursor = (page.len() as i64 == limit)
        .then(|| page.last().cloned())
        .flatten();
    // A malformed ref names a blob that can never be fetched, so listing it
    // would only send a syncer after something that is not there. The write
    // path skips these for the same reason.
    let cids = page
        .into_iter()
        .filter(|cid| <lexicon_cid::Cid as std::str::FromStr>::from_str(cid).is_ok())
        .collect();
    Ok(Json(ListBlobsOutput { cursor, cids }))
}
