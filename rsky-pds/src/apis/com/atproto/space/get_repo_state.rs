use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::get_latest_commit::serve_latest_commit_with_hash;
use crate::apis::com::atproto::space::kem::ActorKemKeys;
use crate::apis::ApiError;
use crate::space_auth::SpaceReadAuth;
use crate::space_mac::{compute_response_mac, MacResponse, MacType, WantMacHeader};
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::GetLatestCommitOutput;

/// `getLatestCommit` under the name implementations shipped before the draft
/// settled on the current one. Clients that know only this spelling 404
/// without it, so it is an alias rather than a second implementation.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getRepoState?<space>&<repo>")]
pub async fn space_get_repo_state(
    space: String,
    repo: String,
    auth: SpaceReadAuth,
    want_mac: WantMacHeader,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<MacResponse<Json<GetLatestCommitOutput>>, ApiError> {
    let (commit, rev, hash) = serve_latest_commit_with_hash(
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
                    "getRepoState requires type=\"atproto-space-repo\", got {:?}",
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
        Json(GetLatestCommitOutput { commit }),
        mac,
        &want_mac,
    ))
}
