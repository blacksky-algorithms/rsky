use crate::account_manager::AccountManager;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::pipethrough::{pipethrough, OverrideOpts, ProxyRequest};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::{make_aturi, ActorStore};
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::GetRecordOutput;

async fn inner_get_record(
    repo: String,
    collection: String,
    rkey: String,
    cid: Option<String>,
    s3_config: &State<SdkConfig>,
    req: ProxyRequest<'_>,
) -> Result<GetRecordOutput> {
    let did = AccountManager::get_did_for_actor(&repo, None).await?;

    // fetch from pds if available, if not then fetch from appview
    if let Some(did) = did {
        let uri = make_aturi(did.clone(), Some(collection), Some(rkey));

        let mut actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));

        match actor_store.record.get_record(&uri, cid, None).await {
            Ok(Some(record)) if record.takedown_ref.is_none() => Ok(GetRecordOutput {
                uri,
                cid: Some(record.cid),
                value: serde_json::to_value(record.value)?,
            }),
            _ => bail!("Could not locate record: `{uri}`"),
        }
    } else {
        match req.cfg.bsky_app_view {
            None => bail!("Could not locate record"),
            Some(_) => match pipethrough(
                &req,
                None,
                OverrideOpts {
                    aud: None,
                    lxm: None,
                },
            )
            .await
            {
                Err(error) => {
                    eprintln!("@LOG: ERROR: {error}");
                    bail!("Could not locate record")
                }
                Ok(res) => {
                    let output: GetRecordOutput = serde_json::from_slice(res.buffer.as_slice())?;
                    Ok(output)
                }
            },
        }
    }
}

#[rocket::get("/xrpc/com.atproto.repo.getRecord?<repo>&<collection>&<rkey>&<cid>")]
pub async fn get_record(
    repo: String,
    collection: String,
    rkey: String,
    cid: Option<String>,
    s3_config: &State<SdkConfig>,
    req: ProxyRequest<'_>,
) -> Result<Json<GetRecordOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_record(repo, collection, rkey, cid, s3_config, req).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let not_found = ErrorMessageResponse {
                code: Some(ErrorCode::NotFound),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(Status::NotFound, Json(not_found)));
        }
    }
}
