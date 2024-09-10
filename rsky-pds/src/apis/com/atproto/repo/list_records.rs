use crate::account_manager::AccountManager;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{ListRecordsOutput, Record};

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
) -> Result<ListRecordsOutput> {
    if limit > 100 {
        bail!("Error: limit can not be greater than 100")
    }
    let did = AccountManager::get_did_for_actor(&repo, None).await?;
    if let Some(did) = did {
        let mut actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));

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
        // @TODO: Use ATUri
        let cursor: Option<String>;
        if let Some(last_record) = last_record {
            let last_uri = last_record.clone().uri;
            let last_uri_without_prefix = last_uri.replace("at://", "");
            let parts = last_uri_without_prefix.split("/").collect::<Vec<&str>>();
            if let (Some(_), Some(_), Some(uri_rkey)) = (parts.get(0), parts.get(1), parts.get(2)) {
                cursor = Some(uri_rkey.to_string());
            } else {
                cursor = None;
            }
        } else {
            cursor = None;
        }
        Ok(ListRecordsOutput { records, cursor })
    } else {
        bail!("Could not find repo: {repo}")
    }
}

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
) -> Result<Json<ListRecordsOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let limit = limit.unwrap_or(50);
    let reverse = reverse.unwrap_or(false);

    match inner_list_records(
        repo, collection, limit, cursor, rkeyStart, rkeyEnd, reverse, s3_config,
    )
    .await
    {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
