use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::SdkConfig;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::ByteStream as AwsStream;
use libipld::Cid;
use rocket::http::Status;
use rocket::response::status;
use rocket::response::stream::ByteStream;
use rocket::serde::json::Json;
use rocket::State;
use std::str::FromStr;

async fn inner_get_blob(
    did: String,
    cid: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<AwsStream> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin).await?;

    let cid = Cid::from_str(&cid)?;
    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));

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
    auth: OptionalAccessOrAdminToken,
) -> Result<ByteStream![Vec<u8>], status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_get_blob(did, cid, s3_config, auth).await {
        Ok(mut stream) => Ok(ByteStream! {
            while let Some(byte_stream) = stream.next().await {
                match byte_stream {
                    Ok(byte_stream) => yield byte_stream.to_vec(),
                    Err(e) => {
                        eprintln!("error while streaming: {}", e);
                        break;
                    }
                }
            }
        }),
        Err(error) => {
            return match error.downcast_ref() {
                Some(GetObjectError::NoSuchKey(_)) => {
                    eprintln!("Error: {}", error);
                    let internal_error = InternalErrorMessageResponse {
                        code: Some(InternalErrorCode::NotFound),
                        message: Some("cannot find blob".to_owned()),
                    };
                    Err(status::Custom(Status::NotFound, Json(internal_error)))
                }
                _ => {
                    eprintln!("Error: {}", error);
                    let internal_error = InternalErrorMessageResponse {
                        code: Some(InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            };
            // @TODO: Need to update error handling to return 404 if we have it but it's in tmp
        }
    }
}