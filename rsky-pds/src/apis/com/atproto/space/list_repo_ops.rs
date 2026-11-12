use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::decode_record;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::get_latest_commit::space_commit;
use crate::apis::com::atproto::space::kem::ActorKemKeys;
use crate::apis::com::atproto::space::{
    internal_error, open_local_repo, parse_space_uri, space_error,
};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_mac::{compute_response_mac, MacResponse, MacType, WantMacHeader};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{RepoOp, SpaceRepoOpsOutput};

/// A repo's operation log since a revision, inlining current record values by
/// default. The terminal page carries the current commit, unsigned and vouched
/// for by the MAC (spec §Incremental sync).
#[tracing::instrument(skip_all)]
#[rocket::get(
    "/xrpc/com.atproto.space.listRepoOps?<space>&<repo>&<since>&<cursor>&<limit>&<excludeValues>"
)]
#[allow(clippy::too_many_arguments, non_snake_case)]
pub async fn space_list_repo_ops(
    space: String,
    repo: String,
    since: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
    excludeValues: Option<bool>,
    auth: SpaceReadAuth,
    want_mac: WantMacHeader,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<MacResponse<Json<SpaceRepoOpsOutput>>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &repo,
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
        &repo,
        false,
        &auth,
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

    // Final page includes the commit, unsigned and vouched for by the MAC;
    // intermediate pages do not
    let (commit, mac_data) = if has_more {
        (None, None)
    } else {
        let state = reader
            .space
            .live_repo_state(&space_id.uri())
            .await
            .map_err(space_error)?;
        let hash = state.hash();
        let rev = state.rev.clone();
        let commit = space_commit(&space_id.uri(), &reader.did, &rev, &hash);
        (Some(commit), Some((rev, hash)))
    };

    // Compute MAC only on final page (when commit is present)
    let mac = if let (Some(ref want), Some((ref rev, ref hash))) = (&want_mac.0, &mac_data) {
        // Validate MAC type matches method
        if want.mac_type != MacType::Commit {
            return Err(ApiError::BadRequest(
                "MacTypeMismatch".to_string(),
                format!(
                    "listRepoOps requires type=\"atproto-space-repo\", got {:?}",
                    want.mac_type
                ),
            ));
        }

        // MAC under the author's `#atproto_kem` key; an account that has none
        // yet answers without a MAC.
        let kem_provider = ActorKemKeys(actor_store);
        compute_response_mac(
            &kem_provider,
            want,
            &repo,
            &space,
            &repo,
            Some(rev),
            Some(hash),
            None,
            None,
        )
        .await
        .ok() // Gracefully handle missing KEM key
    } else {
        None
    };

    Ok(MacResponse::new(
        Json(SpaceRepoOpsOutput {
            cursor: next_cursor,
            ops,
            commit,
        }),
        mac,
        &want_mac,
    ))
}
