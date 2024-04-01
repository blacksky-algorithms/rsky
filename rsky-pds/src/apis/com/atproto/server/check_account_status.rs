use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::is_valid_did_doc_for_service;
use crate::auth_verifier::Access;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::BehaviorVersion;
use futures::try_join;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::CheckAccountStatusOutput;

async fn inner_check_account_status(auth: Access) -> Result<CheckAccountStatusOutput> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;

    let mut actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), &config),
    );
    let (repo_root, repo_blocks, indexed_records, imported_blobs, expected_blobs) = try_join!(
        actor_store.storage.get_root_detailed(),
        actor_store.storage.count_blocks(),
        actor_store.record.record_count(),
        actor_store.blob.blob_count(),
        actor_store.blob.record_blob_count(),
    )?;

    let (activated, valid_did) = try_join!(
        AccountManager::is_account_activated(&requester),
        is_valid_did_doc_for_service(requester.clone())
    )?;

    Ok(CheckAccountStatusOutput {
        activated,
        valid_did,
        repo_commit: repo_root.cid.to_string(),
        repo_rev: repo_root.rev,
        repo_blocks,
        indexed_records,
        private_state_values: 0,
        expected_blobs,
        imported_blobs,
    })
}

#[rocket::get("/xrpc/com.atproto.server.checkAccountStatus")]
pub async fn check_account_status(
    auth: Access,
) -> Result<Json<CheckAccountStatusOutput>, status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_check_account_status(auth).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
