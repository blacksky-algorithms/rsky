use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::db::DbConn;
use crate::pipethrough::{pipethrough, OverrideOpts, ProxyRequest};
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::GetRecordOutput;
use rsky_syntax::aturi::AtUri;

#[tracing::instrument(skip_all)]
async fn inner_get_record(
    repo: String,
    collection: String,
    rkey: String,
    cid: Option<String>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    req: ProxyRequest<'_>,
    account_manager: AccountManager,
) -> Result<GetRecordOutput> {
    let did = account_manager.get_did_for_actor(&repo, None).await?;

    // fetch from pds if available, if not then fetch from appview
    if let Some(did) = did {
        let uri = AtUri::make(did.clone(), Some(collection), Some(rkey))?;

        let mut actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);

        match actor_store.record.get_record(&uri, cid, None).await {
            Ok(Some(record)) if record.takedown_ref.is_none() => Ok(GetRecordOutput {
                uri: uri.to_string(),
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
                    tracing::error!("@LOG: ERROR: {error}");
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

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.repo.getRecord?<repo>&<collection>&<rkey>&<cid>")]
pub async fn get_record(
    repo: String,
    collection: String,
    rkey: String,
    cid: Option<String>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    req: ProxyRequest<'_>,
    account_manager: AccountManager,
) -> Result<Json<GetRecordOutput>, ApiError> {
    match inner_get_record(
        repo,
        collection,
        rkey,
        cid,
        s3_config,
        db,
        req,
        account_manager,
    )
    .await
    {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RecordNotFound)
        }
    }
}
