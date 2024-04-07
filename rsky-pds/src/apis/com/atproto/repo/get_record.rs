use crate::account_manager::AccountManager;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::{make_aturi, ActorStore};
use anyhow::{bail, Result};
use aws_config::BehaviorVersion;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::repo::GetRecordOutput;

async fn inner_get_record(
    repo: String,
    collection: String,
    rkey: String,
    cid: Option<String>,
) -> Result<GetRecordOutput> {
    let did = AccountManager::get_did_for_actor(&repo, None).await?;

    // fetch from pds if available, if not then fetch from appview
    if let Some(did) = did {
        // @TODO: Use ATUri
        let uri = make_aturi(did.clone(), Some(collection), Some(rkey));
        let config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;

        let mut actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), &config));

        match actor_store.record.get_record(&uri, cid, None).await {
            Ok(Some(record)) if record.takedown_ref.is_none() => Ok(GetRecordOutput {
                uri,
                cid: Some(record.cid),
                value: serde_json::to_value(record.value)?,
            }),
            _ => bail!("Could not locate record: `{uri}`"),
        }
    } else {
        // @TODO: Passthrough to Bsky AppView
        bail!("Could not locate record")
    }
}

#[rocket::get("/xrpc/com.atproto.repo.getRecord?<repo>&<collection>&<rkey>&<cid>")]
pub async fn get_record(
    repo: String,
    collection: String,
    rkey: String,
    cid: Option<String>,
) -> Result<Json<GetRecordOutput>, status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_get_record(repo, collection, rkey, cid).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
