use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{open_local_repo, parse_space_uri, serve_commit};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::GetLatestCommitOutput;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getLatestCommit?<space>&<did>")]
pub async fn space_get_latest_commit(
    space: String,
    did: String,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<Json<GetLatestCommitOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &did,
        &SpaceRequest::ReadSelf { collection: None },
    )?;
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &did,
        false,
    )
    .await?;
    let commit = serve_commit(&reader, &space_id.uri()).await?;
    Ok(Json(GetLatestCommitOutput { commit }))
}
