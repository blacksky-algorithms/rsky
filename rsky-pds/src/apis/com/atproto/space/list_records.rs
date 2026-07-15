use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::decode_record;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{
    internal_error, open_local_repo, parse_space_uri, space_error,
};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{ListRecordsOutput, Record};

#[tracing::instrument(skip_all)]
#[rocket::get(
    "/xrpc/com.atproto.space.listRecords?<space>&<did>&<collection>&<limit>&<cursor>&<excludeValues>"
)]
#[allow(clippy::too_many_arguments, non_snake_case)]
pub async fn space_list_records(
    space: String,
    did: String,
    collection: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
    excludeValues: Option<bool>,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<Json<ListRecordsOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &did,
        &SpaceRequest::ReadSelf {
            collection: collection.clone(),
        },
    )?;
    let limit = limit.unwrap_or(50).clamp(1, 100) as usize;
    let exclude_values = excludeValues.unwrap_or(false);
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
    let rows = reader
        .space
        .list_records(&space_id.uri(), collection, limit, cursor)
        .await
        .map_err(space_error)?;
    let cursor = if rows.len() == limit {
        rows.last()
            .map(|row| format!("{}/{}", row.collection, row.rkey))
    } else {
        None
    };
    let records = rows
        .into_iter()
        .map(|row| {
            let value = if exclude_values {
                None
            } else {
                Some(
                    decode_record(&row.value)
                        .map_err(internal_error("stored record failed to decode"))?,
                )
            };
            Ok(Record {
                uri: space_id.record_uri(&did, &row.collection, &row.rkey),
                cid: row.cid,
                value,
            })
        })
        .collect::<Result<Vec<Record>, ApiError>>()?;
    Ok(Json(ListRecordsOutput { cursor, records }))
}
