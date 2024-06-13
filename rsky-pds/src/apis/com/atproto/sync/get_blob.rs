use std::str::FromStr;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use anyhow::{Result};
use aws_config::SdkConfig;
use aws_sdk_s3::primitives::ByteStream as AwsStream;
use rocket::response::stream::ByteStream;
use libipld::Cid;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::auth_verifier;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::repo::ActorStore;
use crate::repo::aws::s3::S3BlobStore;

async fn inner_get_blob(
    did: String,
    cid: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken
) -> Result<AwsStream> {
    let _ = assert_repo_availability(
        &did,
        auth_verifier::is_user_or_admin(auth.access.unwrap(), &did)
    ).await?;

    let cid = Cid::from_str(&cid)?;
    let actor_store = ActorStore::new(
        did.clone(),
        S3BlobStore::new(did.clone(), s3_config),
    );

    let found = actor_store.blob.get_blob(cid).await?;
    Ok(found.stream)
}

/// Get a blob associated with a given account. Returns the full blob as originally uploaded.
/// Does not require auth; implemented by PDS.
#[rocket::get("/xrpc/com.atproto.sync.getBlob?<did>&<cid>")]
pub async fn get_blob(
    did: String,
    cid: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken
) -> Result<ByteStream![Vec<u8>], status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_get_blob(did, cid, s3_config, auth).await {
        Ok(mut stream) => {
            Ok(ByteStream! {
                while let Some(byte_stream) = stream.next().await {
                    match byte_stream {
                        Ok(byte_stream) => yield byte_stream.to_vec(),
                        Err(e) => {
                            eprintln!("error while streaming: {}", e);
                            break;
                        }
                    }
                }
            })
        },
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
