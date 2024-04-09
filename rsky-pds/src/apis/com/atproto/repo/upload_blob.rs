use crate::auth_verifier::AccessCheckTakedown;
use crate::common::ContentType;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::data::{Data, ToByteUnit};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::BlobOutput;

async fn inner_upload_blob(
    auth: AccessCheckTakedown,
    blob: Data<'_>,
    content_type: ContentType,
    s3_config: &State<SdkConfig>,
) -> Result<()> {
    let requester = auth.access.credentials.unwrap().did.unwrap();

    let mut actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
    );

    let bytes = blob.open(100.mebibytes()).into_bytes().await?;
    if !bytes.is_complete() {
        println!("there are bytes remaining in the stream");
    }
    let blob_len = bytes.len();

    println!("File Size: {blob_len}");
    println!("File Type: {:?}", content_type.name);

    let key = actor_store
        .blob
        .blobstore
        .put_temp(bytes.into_inner())
        .await?;
    println!("Upload successful: {key}");
    Ok(())
}

#[rocket::post("/xrpc/com.atproto.repo.uploadBlob", data = "<blob>")]
pub async fn upload_blob(
    auth: AccessCheckTakedown,
    blob: Data<'_>,
    content_type: ContentType,
    s3_config: &State<SdkConfig>,
) -> Result<() /*Json<BlobOutput>*/, status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_upload_blob(auth, blob, content_type, s3_config).await {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("{error:?}");
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
