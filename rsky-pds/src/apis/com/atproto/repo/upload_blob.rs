use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::common::ContentType;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::types::{BlobConstraint, PreparedBlobRef};
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::data::Data;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{Blob, BlobOutput};

async fn inner_upload_blob(
    auth: AccessStandardIncludeChecks,
    blob: Data<'_>,
    content_type: ContentType,
    s3_config: &State<SdkConfig>,
) -> Result<BlobOutput> {
    let requester = auth.access.credentials.unwrap().did.unwrap();

    let actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
    );

    let metadata = actor_store
        .blob
        .upload_blob_and_get_metadata(content_type.name, blob)
        .await?;
    let blobref = actor_store.blob.track_untethered_blob(metadata).await?;

    // make the blob permanent if an associated record is already indexed
    let records_for_blob = actor_store
        .blob
        .get_records_for_blob(blobref.get_cid()?)
        .await?;

    if records_for_blob.len() > 0 {
        let _ = actor_store
            .blob
            .verify_blob_and_make_permanent(PreparedBlobRef {
                cid: blobref.get_cid()?,
                mime_type: blobref.get_mime_type().to_string(),
                constraints: BlobConstraint {
                    max_size: None,
                    accept: None,
                },
            })
            .await?;
    }

    Ok(BlobOutput {
        blob: Blob {
            r#type: Some("blob".to_string()),
            r#ref: Some(blobref.get_cid()?),
            cid: None,
            mime_type: blobref.get_mime_type().to_string(),
            size: blobref.get_size(),
            original: None,
        },
    })
}

#[rocket::post("/xrpc/com.atproto.repo.uploadBlob", data = "<blob>")]
pub async fn upload_blob(
    auth: AccessStandardIncludeChecks,
    blob: Data<'_>,
    content_type: ContentType,
    s3_config: &State<SdkConfig>,
) -> Result<Json<BlobOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_upload_blob(auth, blob, content_type, s3_config).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("{error:?}");
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
