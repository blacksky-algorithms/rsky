use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::decode_record;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{open_local_repo, parse_space_uri, space_error};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::GetRecordOutput;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getRecord?<space>&<did>&<collection>&<rkey>&<cid>")]
#[allow(clippy::too_many_arguments)]
pub async fn space_get_record(
    space: String,
    did: String,
    collection: String,
    rkey: String,
    cid: Option<String>,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<Json<GetRecordOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &did,
        &SpaceRequest::ReadSelf {
            collection: Some(collection.clone()),
        },
    )?;
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &did,
        false,
    )
    .await?;
    reader
        .space
        .live_repo_state(&space_id.uri())
        .await
        .map_err(space_error)?;
    let record = reader
        .space
        .get_record(&space_id.uri(), &collection, &rkey)
        .await
        .map_err(space_error)?
        .ok_or(ApiError::RecordNotFound)?;
    if let Some(cid) = cid {
        if record.cid != cid {
            return Err(ApiError::RecordNotFound);
        }
    }
    let value = decode_record(&record.value).map_err(|error| {
        tracing::error!("stored record failed to decode: {error}");
        ApiError::RuntimeError
    })?;
    Ok(Json(GetRecordOutput {
        uri: space_id.record_uri(&did, &collection, &rkey),
        cid: Some(record.cid),
        value,
    }))
}
