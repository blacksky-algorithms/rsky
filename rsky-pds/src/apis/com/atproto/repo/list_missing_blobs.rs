use crate::actor_store::blob::ListMissingBlobsOpts;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use anyhow::Result;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::ListMissingBlobsOutput;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.repo.listMissingBlobs?<limit>&<cursor>")]
pub async fn list_missing_blobs(
    limit: Option<u16>,
    cursor: Option<String>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<ListMissingBlobsOutput>, ApiError> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let limit: u16 = limit.unwrap_or(500);

    let actor_store = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await?;

    match actor_store
        .blob
        .list_missing_blobs(ListMissingBlobsOpts { cursor, limit })
        .await
    {
        Ok(blobs) => {
            let cursor = blobs.last().map(|last_blob| last_blob.cid.clone());
            Ok(Json(ListMissingBlobsOutput { cursor, blobs }))
        }
        Err(error) => {
            tracing::error!("{error:?}");
            Err(ApiError::RuntimeError)
        }
    }
}
