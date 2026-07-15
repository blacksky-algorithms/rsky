use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use anyhow::Result;
use rocket::data::{Data, ToByteUnit};
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
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
) -> Result<BlobOutput> {
    let requester = auth.access.credentials.unwrap().did.unwrap();

    let bytes = blob.open(100.mebibytes()).into_bytes().await?.into_inner();
    let actor_store = actor_store
        .transact(
            requester.clone(),
            blobstore_factory.blobstore(requester.clone()),
        )
        .await?;

    let metadata = actor_store
        .blob
        .upload_blob_and_get_metadata(content_type.name, bytes)
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

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.repo.uploadBlob", data = "<blob>")]
pub async fn upload_blob(
    auth: AccessStandardIncludeChecks,
    blob: Data<'_>,
    content_type: ContentType,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
) -> Result<Json<BlobOutput>, ApiError> {
    match inner_upload_blob(auth, blob, content_type, blobstore_factory, actor_store).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("{error:?}");
            Err(ApiError::RuntimeError)
        }
    }
}
