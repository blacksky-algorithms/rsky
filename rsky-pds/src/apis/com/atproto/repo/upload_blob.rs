use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::db::DbConn;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::data::Data;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use rocket::serde::json::Json;
use rocket::{Request, State};
use rsky_common::BadContentTypeError;
use rsky_lexicon::com::atproto::repo::{Blob, BlobOutput};
use rsky_repo::types::{BlobConstraint, PreparedBlobRef};

#[derive(Clone)]
pub struct ContentType {
    pub name: String,
}

/// Used mainly as a way to parse out content-type from request
#[rocket::async_trait]
impl<'r> FromRequest<'r> for ContentType {
    type Error = BadContentTypeError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match req.content_type() {
            None => Outcome::Error((
                Status::UnsupportedMediaType,
                BadContentTypeError::MissingType,
            )),
            Some(content_type) => Outcome::Success(ContentType {
                name: content_type.to_string(),
            }),
        }
    }
}

async fn inner_upload_blob(
    auth: AccessStandardIncludeChecks,
    blob: Data<'_>,
    content_type: ContentType,
    s3_config: &State<SdkConfig>,
    db: DbConn,
) -> Result<BlobOutput> {
    let requester = auth.access.credentials.unwrap().did.unwrap();

    let actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
        db,
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

    if !records_for_blob.is_empty() {
        actor_store
            .blob
            .verify_blob_and_make_permanent_legacy(PreparedBlobRef {
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

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.repo.uploadBlob", data = "<blob>")]
pub async fn upload_blob(
    auth: AccessStandardIncludeChecks,
    blob: Data<'_>,
    content_type: ContentType,
    s3_config: &State<SdkConfig>,
    db: DbConn,
) -> Result<Json<BlobOutput>, ApiError> {
    match inner_upload_blob(auth, blob, content_type, s3_config, db).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("{error:?}");
            Err(ApiError::RuntimeError)
        }
    }
}
