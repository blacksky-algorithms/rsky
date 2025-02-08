use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::db::DbConn;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{ListRecordsOutput, Record};
use rsky_syntax::aturi::AtUri;

#[allow(non_snake_case)]
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
    s3_config: &State<SdkConfig>,
    db: DbConn,
) -> Result<ListRecordsOutput> {
    if limit > 100 {
        bail!("Error: limit can not be greater than 100")
    }
    let did = AccountManager::get_did_for_actor(&repo, None).await?;
    if let Some(did) = did {
        let mut actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);

        let records: Vec<Record> = actor_store
            .record
            .list_records_for_collection(
                collection,
                limit as i64,
                reverse,
                cursor,
                rkeyStart,
                rkeyEnd,
                None,
            )
            .await?
            .into_iter()
            .map(|record| {
                Ok(Record {
                    uri: record.uri.clone(),
                    cid: record.cid.clone(),
                    value: serde_json::to_value(record)?,
                })
            })
            .collect::<Result<Vec<Record>>>()?;

        let last_record = records.last();
        let cursor: Option<String>;
        if let Some(last_record) = last_record {
            let last_at_uri: AtUri = last_record.uri.clone().try_into()?;
            cursor = Some(last_at_uri.get_rkey());
        } else {
            cursor = None;
        }
        Ok(ListRecordsOutput { records, cursor })
    } else {
        bail!("Could not find repo: {repo}")
    }
}

#[tracing::instrument(skip_all)]
#[allow(non_snake_case)]
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
    s3_config: &State<SdkConfig>,
    db: DbConn,
) -> Result<Json<ListRecordsOutput>, ApiError> {
    let limit = limit.unwrap_or(50);
    let reverse = reverse.unwrap_or(false);

    match inner_list_records(
        repo, collection, limit, cursor, rkeyStart, rkeyEnd, reverse, s3_config, db,
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
