use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::SdkConfig;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::AggregatedBytes;
use libipld::Cid;
use rocket::http::Header;
use rocket::{Responder, State};
use std::str::FromStr;

#[derive(Responder)]
#[response(status = 200)]
pub struct BlobResponder(Vec<u8>, Header<'static>, Header<'static>, Header<'static>);

async fn inner_get_blob(
    did: String,
    cid: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<(Vec<u8>, Option<String>)> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin).await?;

    let cid = Cid::from_str(&cid)?;
    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));

    let found = actor_store.blob.get_blob(cid).await?;
    let buf: AggregatedBytes = found.stream.collect().await?;
    Ok((buf.to_vec(), found.mime_type))
}

/// Get a blob associated with a given account. Returns the full blob as originally uploaded.
/// Does not require auth; implemented by PDS.
#[rocket::get("/xrpc/com.atproto.sync.getBlob?<did>&<cid>")]
pub async fn get_blob(
    did: String,
    cid: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<BlobResponder, ApiError> {
    match inner_get_blob(did, cid, s3_config, auth).await {
        Ok(res) => {
            let (bytes, mime_type) = res;
            Ok(BlobResponder(
                bytes.clone(),
                Header::new("content-length", bytes.len().to_string()),
                Header::new(
                    "content-type",
                    mime_type.unwrap_or("application/octet-stream".to_string()),
                ),
                Header::new("content-security-policy", "default-src 'none'; sandbox"),
            ))
        }
        Err(error) => {
            match error.downcast_ref() {
                Some(GetObjectError::NoSuchKey(_)) => {
                    eprintln!("Error: {}", error);
                    Err(ApiError::BlobNotFound)
                }
                _ => {
                    eprintln!("Error: {}", error);
                    Err(ApiError::RuntimeError)
                }
            }
            // @TODO: Need to update error handling to return 404 if we have it but it's in tmp
        }
    }
}
