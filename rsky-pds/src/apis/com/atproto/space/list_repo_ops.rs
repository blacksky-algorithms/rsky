use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::decode_record;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{
    internal_error, open_local_repo, parse_space_uri, serve_commit, space_error,
};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{ListRepoOpsOutput, RepoOp};

/// A repo's operation log since a revision, inlining current record values by
/// default. The terminal page carries the current signed commit
/// (spec §Incremental sync).
#[tracing::instrument(skip_all)]
#[rocket::get(
    "/xrpc/com.atproto.space.listRepoOps?<space>&<did>&<since>&<cursor>&<limit>&<excludeValues>"
)]
#[allow(clippy::too_many_arguments, non_snake_case)]
pub async fn space_list_repo_ops(
    space: String,
    did: String,
    since: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
    excludeValues: Option<bool>,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<Json<ListRepoOpsOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &did,
        &SpaceRequest::ReadSelf { collection: None },
    )?;
    let limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
    let exclude_values = excludeValues.unwrap_or(false);
    let cursor = match cursor {
        Some(cursor) => Some(
            cursor
                .parse::<i64>()
                .map_err(|_| ApiError::InvalidRequest(format!("invalid cursor: {cursor}")))?,
        ),
        None => None,
    };
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &did,
        false,
    )
    .await?;
    let (rows, has_more) = reader
        .space
        .list_repo_ops(&space_id.uri(), since, cursor, limit)
        .await
        .map_err(space_error)?;
    let next_cursor = if has_more {
        rows.last().map(|row| row.id.to_string())
    } else {
        None
    };
    let ops = rows
        .into_iter()
        .map(|row| {
            let value = match (&row.value, exclude_values) {
                (Some(bytes), false) => Some(
                    decode_record(bytes)
                        .map_err(internal_error("stored record failed to decode"))?,
                ),
                _ => None,
            };
            Ok(RepoOp {
                rev: row.rev,
                collection: row.collection,
                rkey: row.rkey,
                cid: row.cid,
                prev: row.prev,
                value,
            })
        })
        .collect::<Result<Vec<RepoOp>, ApiError>>()?;
    let commit = if has_more {
        None
    } else {
        Some(serve_commit(&reader, &space_id.uri()).await?)
    };
    Ok(Json(ListRepoOpsOutput {
        cursor: next_cursor,
        ops,
        commit,
    }))
}
