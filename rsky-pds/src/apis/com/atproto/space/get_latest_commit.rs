use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::kem::ActorKemKeys;
use crate::apis::com::atproto::space::{open_local_repo, parse_space_uri, space_error};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_mac::{compute_response_mac, MacResponse, MacType, WantMacHeader};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{LatestSpaceCommitOutput, SpaceCommit};

use crate::actor_store::space::commit::{sign_commit, to_lexicon, COMMIT_VERSION};
use crate::apis::com::atproto::space::internal_error;

/// Serve the latest commit for a repo, returning the signed commit and the hash
/// (needed for MAC computation).
pub async fn serve_latest_commit_with_hash(
    space: String,
    repo: String,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<
    (
        rsky_lexicon::com::atproto::space::SignedCommit,
        String,
        [u8; 32],
    ),
    ApiError,
> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &repo,
        &SpaceRequest::ReadSelf { collection: None },
    )?;
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &repo,
        false,
        &auth,
    )
    .await?;
    let state = reader
        .space
        .live_repo_state(&space_id.uri())
        .await
        .map_err(space_error)?;
    let hash = state.hash();
    let keypair = reader
        .keypair()
        .await
        .map_err(internal_error("missing actor keypair"))?;
    let commit = sign_commit(&keypair, &space_id.uri(), &reader.did, &state.rev, &hash)
        .map_err(internal_error("commit signing failed"))?;
    let rev = commit.rev.clone();
    Ok((to_lexicon(&commit), rev, hash))
}

/// A repo's latest commit, unsigned (2026-10-22 spaces alpha), with its rev and
/// hash for the MAC that vouches for it. Needs no signing key.
pub async fn serve_latest_space_commit(
    space: String,
    repo: String,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<(SpaceCommit, String, [u8; 32]), ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &repo,
        &SpaceRequest::ReadSelf { collection: None },
    )?;
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &repo,
        false,
        &auth,
    )
    .await?;
    let state = reader
        .space
        .live_repo_state(&space_id.uri())
        .await
        .map_err(space_error)?;
    let hash = state.hash();
    let commit = space_commit(&space_id.uri(), &reader.did, &state.rev, &hash);
    Ok((commit, state.rev, hash))
}

/// The unsigned commit for a repo state.
pub fn space_commit(space: &str, repo: &str, rev: &str, hash: &[u8; 32]) -> SpaceCommit {
    SpaceCommit {
        ver: i64::from(COMMIT_VERSION),
        repo: repo.to_string(),
        space: space.to_string(),
        rev: rev.to_string(),
        data: hash.to_vec(),
    }
}

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getLatestCommit?<space>&<repo>")]
pub async fn space_get_latest_commit(
    space: String,
    repo: String,
    auth: SpaceReadAuth,
    want_mac: WantMacHeader,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<MacResponse<Json<LatestSpaceCommitOutput>>, ApiError> {
    let (commit, rev, hash) = serve_latest_space_commit(
        space.clone(),
        repo.clone(),
        auth,
        actor_store,
        blobstore_factory,
        account_manager,
    )
    .await?;

    // Compute MAC if requested
    let mac = if let Some(ref want) = want_mac.0 {
        // Validate MAC type matches method
        if want.mac_type != MacType::Commit {
            return Err(ApiError::BadRequest(
                "MacTypeMismatch".to_string(),
                format!(
                    "getLatestCommit requires type=\"atproto-space-repo\", got {:?}",
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
            Some(&rev),
            Some(&hash),
            None,
            None,
        )
        .await
        .ok() // Gracefully handle missing KEM key
    } else {
        None
    };

    Ok(MacResponse::new(
        Json(LatestSpaceCommitOutput { commit }),
        mac,
        &want_mac,
    ))
}
