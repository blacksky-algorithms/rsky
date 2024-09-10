use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::car::read_car_bytes;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use libipld::Cid;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{Responder, State};
use std::str::FromStr;

#[derive(Responder)]
#[response(status = 200, content_type = "application/vnd.ipld.car")]
pub struct BlockResponder(Vec<u8>);

async fn inner_get_blocks(
    did: String,
    cids: Vec<String>,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<Vec<u8>> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin).await?;

    let cids: Vec<Cid> = cids
        .into_iter()
        .map(|c| Cid::from_str(&c).map_err(anyhow::Error::new))
        .collect::<Result<Vec<Cid>>>()?;

    let mut actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
    let got = actor_store.storage.get_blocks(cids).await?;

    if got.missing.len() > 0 {
        let missing_str = got
            .missing
            .into_iter()
            .map(|c| c.to_string())
            .collect::<Vec<String>>();
        bail!("Could not find cids: `{missing_str:?}`");
    }

    let car = read_car_bytes(None, got.blocks).await?;
    Ok(car)
}

/// Get data blocks from a given repo, by CID. For example, intermediate MST nodes, or records.
/// Does not require auth; implemented by PDS.
#[rocket::get("/xrpc/com.atproto.sync.getBlocks?<did>&<cids>")]
pub async fn get_blocks(
    did: String,
    cids: Vec<String>,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<BlockResponder, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_blocks(did, cids, s3_config, auth).await {
        Ok(res) => Ok(BlockResponder(res)),
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
