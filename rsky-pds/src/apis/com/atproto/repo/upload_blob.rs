use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{BlobUpload, Scoped};
use crate::auth_verifier::AccessOrUserServiceAuth;
use crate::config::ServerConfig;
use crate::metrics::record_blob_upload;
use crate::rate_limits::{Caller, RateLimits};
use crate::spool::SpoolFile;
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
    auth: Scoped<BlobUpload, AccessOrUserServiceAuth>,
    blob: Data<'_>,
    content_type: ContentType,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    cfg: &State<ServerConfig>,
) -> Result<BlobOutput, ApiError> {
    let requester = auth.did().await?;

    // spooled to disk, one byte past the limit, so a body over the limit
    // is refused without ever being held in memory
    let limit = cfg.service.blob_upload_limit as u64;
    let spool = SpoolFile::new(&cfg.service.upload_spool_dir).await?;
    let written = blob
        .open((limit + 1).bytes())
        .into_file(spool.path())
        .await
        .map_err(anyhow::Error::from)?;
    if written.n.written > limit {
        return Err(ApiError::PayloadTooLarge);
    }
    let actor_store = actor_store
        .transact(
            requester.clone(),
            blobstore_factory.blobstore(requester.clone()),
        )
        .await?;

    let metadata = actor_store
        .blob
        .upload_blob_from_path(content_type.name, spool.path().to_path_buf())
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

    record_blob_upload(blobref.get_size().unwrap_or(0).max(0) as u64);

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

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.repo.uploadBlob", data = "<blob>")]
pub async fn upload_blob(
    // `content_type` first so a request with no content type gets its own
    // guard's rejection before the `blob:` scope check re-reads the same
    // header.
    content_type: ContentType,
    auth: Scoped<BlobUpload, AccessOrUserServiceAuth>,
    blob: Data<'_>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    cfg: &State<ServerConfig>,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Json<BlobOutput>, ApiError> {
    limits
        .consume_all(
            &crate::rate_limits::UPLOAD_BLOB,
            &caller.ip,
            1,
            caller.bypass,
        )
        .await?;
    match inner_upload_blob(
        auth,
        blob,
        content_type,
        blobstore_factory,
        actor_store,
        cfg,
    )
    .await
    {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("{error:?}");
            Err(error)
        }
    }
}
