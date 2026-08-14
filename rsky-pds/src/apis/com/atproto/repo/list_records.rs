use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{ListRecordsOutput, Record};
use rsky_syntax::aturi::AtUri;

#[allow(non_snake_case, clippy::too_many_arguments)]
async fn inner_list_records(
    // The handle or DID of the repo.
    repo: String,
    // The NSID of the record type.
    collection: String,
    // The number of records to return.
    limit: u16,
    cursor: Option<String>,
    // DEPRECATED: The lowest sort-ordered rkey to start from (exclusive)
    rkeyStart: Option<String>,
    // DEPRECATED: The highest sort-ordered rkey to stop at (exclusive)
    rkeyEnd: Option<String>,
    // Flag to reverse the order of the returned records.
    reverse: bool,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<ListRecordsOutput> {
    if limit > 100 {
        bail!("Error: limit can not be greater than 100")
    }
    let did = account_manager.get_did_for_actor(&repo, None).await?;
    if let Some(did) = did {
        let mut actor_store = actor_store
            .read(did.clone(), blobstore_factory.blobstore(did.clone()))
            .await?;

        // Fetch one extra row to determine whether this page can be resumed.
        // Returning a cursor after a short final page makes compliant clients
        // issue a pointless empty request, and was observed in Bulleted.
        let mut rows = actor_store
            .record
            .list_records_for_collection(
                collection,
                i64::from(limit) + 1,
                reverse,
                cursor,
                rkeyStart,
                rkeyEnd,
                None,
            )
            .await?;
        let has_more = rows.len() > usize::from(limit);
        rows.truncate(usize::from(limit));
        let records: Vec<Record> = rows
            .into_iter()
            .map(|record| {
                Ok(Record {
                    uri: record.uri.clone(),
                    cid: record.cid.clone(),
                    // The record body only. Serializing the whole row here
                    // double-wraps it as {uri, cid, value}, which no atproto
                    // client can read; getRecord serializes record.value too.
                    value: serde_json::to_value(record.value)?,
                })
            })
            .collect::<Result<Vec<Record>>>()?;

        let cursor = if has_more {
            let last_record = records
                .last()
                .expect("a page with another record has a last record");
            let last_at_uri: AtUri = last_record.uri.clone().try_into()?;
            Some(last_at_uri.get_rkey())
        } else {
            None
        };
        Ok(ListRecordsOutput { records, cursor })
    } else {
        bail!("Could not find repo: {repo}")
    }
}

#[tracing::instrument(skip_all)]
#[allow(non_snake_case, clippy::too_many_arguments)]
#[rocket::get("/xrpc/com.atproto.repo.listRecords?<repo>&<collection>&<limit>&<cursor>&<rkeyStart>&<rkeyEnd>&<reverse>")]
pub async fn list_records(
    // The handle or DID of the repo.
    repo: String,
    // The NSID of the record type.
    collection: String,
    // The number of records to return.
    limit: Option<u16>,
    cursor: Option<String>,
    // DEPRECATED: The lowest sort-ordered rkey to start from (exclusive)
    rkeyStart: Option<String>,
    // DEPRECATED: The highest sort-ordered rkey to stop at (exclusive)
    rkeyEnd: Option<String>,
    // Flag to reverse the order of the returned records.
    reverse: Option<bool>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<Json<ListRecordsOutput>, ApiError> {
    let limit = limit.unwrap_or(50);
    let reverse = reverse.unwrap_or(false);

    match inner_list_records(
        repo,
        collection,
        limit,
        cursor,
        rkeyStart,
        rkeyEnd,
        reverse,
        blobstore_factory,
        actor_store,
        account_manager,
    )
    .await
    {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
