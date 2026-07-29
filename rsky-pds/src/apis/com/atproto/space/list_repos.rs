use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::local_space_def;
use crate::apis::com::atproto::space::{parse_space_uri, space_error};
use crate::apis::ApiError;
use crate::space_auth::SpaceCredentialAuth;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{ListReposOutput, RepoRef};

/// The writer set: accounts known to hold data in the space, with each repo's
/// current rev and commit hash (space-host role).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.listRepos?<space>&<limit>&<cursor>")]
pub async fn space_list_repos(
    space: String,
    limit: Option<i64>,
    cursor: Option<String>,
    auth: SpaceCredentialAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<ListReposOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    if auth.space_uri != space_id.uri() {
        return Err(ApiError::InvalidToken);
    }
    let limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
    let (_, space_store, _) = local_space_def(actor_store, blobstore_factory, &space_id).await?;
    let writers = space_store
        .list_writers(&space_id.uri(), limit, cursor)
        .await
        .map_err(space_error)?;
    let cursor = if writers.len() == limit {
        writers.last().map(|writer| writer.did.clone())
    } else {
        None
    };
    let repos = writers
        .into_iter()
        .map(|writer| RepoRef {
            did: writer.did,
            rev: writer.rev,
            hash: writer.hash,
        })
        .collect();
    Ok(Json(ListReposOutput { cursor, repos }))
}
