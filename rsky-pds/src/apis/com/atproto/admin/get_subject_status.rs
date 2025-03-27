use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use crate::db::DbConn;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use futures::try_join;
use libipld::Cid;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::admin::{RepoBlobRef, RepoRef, Subject, SubjectStatus};
use rsky_lexicon::com::atproto::repo::StrongRef;
use std::str::FromStr;

async fn inner_get_subject_status(
    did: Option<String>,
    uri: Option<String>,
    blob: Option<String>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<SubjectStatus> {
    let mut body: Option<SubjectStatus> = None;
    if let Some(blob) = blob {
        match did {
            None => bail!("Must provide a did to request blob state"),
            Some(did) => {
                let actor_store =
                    ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);

                let takedown = actor_store
                    .blob
                    .get_blob_takedown_status(Cid::from_str(&blob)?)
                    .await?;
                if let Some(takedown) = takedown {
                    body = Some(SubjectStatus {
                        subject: Subject::RepoBlobRef(RepoBlobRef {
                            did,
                            cid: blob,
                            record_uri: None,
                        }),
                        takedown: Some(takedown),
                        deactivated: None,
                    });
                }
            }
        }
    } else if let Some(uri) = uri {
        let uri_without_prefix = uri.replace("at://", "");
        let parts = uri_without_prefix.split("/").collect::<Vec<&str>>();
        if let (Some(uri_hostname), Some(_), Some(_)) = (parts.first(), parts.get(1), parts.get(2))
        {
            let actor_store = ActorStore::new(
                uri_hostname.to_string(),
                S3BlobStore::new(uri_hostname.to_string(), s3_config),
                db,
            );
            let (takedown, cid) = try_join!(
                actor_store.record.get_record_takedown_status(uri.clone()),
                actor_store.record.get_current_record_cid(uri.clone()),
            )?;
            if let (Some(cid), Some(takedown)) = (cid, takedown) {
                body = Some(SubjectStatus {
                    subject: Subject::StrongRef(StrongRef {
                        uri,
                        cid: cid.to_string(),
                    }),
                    takedown: Some(takedown),
                    deactivated: None,
                });
            }
        }
    } else if let Some(did) = did {
        let status = account_manager.get_account_admin_status(&did).await?;
        if let Some(status) = status {
            body = Some(SubjectStatus {
                subject: Subject::RepoRef(RepoRef { did }),
                takedown: Some(status.takedown),
                deactivated: Some(status.deactivated),
            });
        }
    } else {
        bail!("No provided subject");
    }
    match body {
        None => bail!("NotFound: Subject not found"),
        Some(body) => Ok(body),
    }
}

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.admin.getSubjectStatus?<did>&<uri>&<blob>")]
pub async fn get_subject_status(
    did: Option<String>,
    uri: Option<String>,
    blob: Option<String>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    _auth: Moderator,
    account_manager: AccountManager,
) -> Result<Json<SubjectStatus>, ApiError> {
    match inner_get_subject_status(did, uri, blob, s3_config, db, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            return Err(ApiError::RuntimeError);
        }
    }
}
