use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::local_space_def;
use crate::apis::com::atproto::space::{parse_space_uri, space_error};
use crate::apis::ApiError;
use crate::space_auth::SpaceCredentialAuth;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{WriterRef, WriterSetOutput};

/// The writer set as a feed (space-host role, 2026-10-01 spaces alpha): the
/// repos updated after space revision `cursor`, oldest first, each with its
/// latest repo revision, commit `data`, and space revision. `cursor` in the
/// answer is the last entry's space revision, on every non-empty page; an
/// empty page means the caller has caught up.
///
/// A writer recorded without a digest -- one known only from a write
/// notification that carried none -- is reported with an all-zero `data`. The
/// field is advisory: a mismatch is a syncer's reason to fetch the repo's head
/// commit, never proof of anything, so the zero digest asks for exactly that.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.listRepos?<space>&<limit>&<cursor>")]
pub async fn space_list_repos(
    space: String,
    limit: Option<i64>,
    cursor: Option<String>,
    auth: SpaceCredentialAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<WriterSetOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    if auth.space_uri != space_id.uri() {
        return Err(ApiError::InvalidToken("Token is invalid".to_string()));
    }
    let limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
    let (_, space_store, _) = local_space_def(actor_store, blobstore_factory, &space_id).await?;
    let writers = space_store
        .list_writers(&space_id.uri(), limit, cursor)
        .await
        .map_err(space_error)?;
    let cursor = writers.last().map(|writer| writer.space_rev.clone());
    let repos = writers
        .into_iter()
        .map(|writer| WriterRef {
            did: writer.did,
            repo_rev: writer.rev,
            data: writer
                .hash
                .as_deref()
                .and_then(|hash| hex::decode(hash).ok())
                .unwrap_or_else(|| vec![0u8; 32]),
            space_rev: writer.space_rev,
        })
        .collect();
    Ok(Json(WriterSetOutput { cursor, repos }))
}
