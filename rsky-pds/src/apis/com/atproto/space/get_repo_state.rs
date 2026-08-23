use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::get_latest_commit::serve_latest_commit;
use crate::apis::ApiError;
use crate::space_auth::SpaceReadAuth;
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
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<Json<GetLatestCommitOutput>, ApiError> {
    serve_latest_commit(
        space,
        repo,
        auth,
        actor_store,
        blobstore_factory,
        account_manager,
    )
    .await
}
