use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::types::{PreparedDelete, PreparedWrite};
use crate::repo::{
    prepare_create, prepare_delete, ActorStore, PrepareCreateOpts, PrepareDeleteOpts,
};
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use libipld::Cid;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{CreateRecordInput, CreateRecordOutput};
use std::str::FromStr;

async fn inner_create_record(
    body: Json<CreateRecordInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<CreateRecordOutput> {
    let CreateRecordInput {
        repo,
        collection,
        record,
        rkey,
        validate,
        swap_commit,
    } = body.into_inner();
    let account = AccountManager::get_account(
        &repo,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: None,
        }),
    )
    .await?;
    if let Some(account) = account {
        if account.deactivated_at.is_some() {
            bail!("Account is deactivated")
        }
        let did = account.did;
        if did != auth.access.credentials.unwrap().did.unwrap() {
            bail!("AuthRequiredError")
        }
        let swap_commit_cid = match swap_commit {
            Some(swap_commit) => Some(Cid::from_str(&swap_commit)?),
            None => None,
        };
        let write = prepare_create(PrepareCreateOpts {
            did: did.clone(),
            collection: collection.clone(),
            record: serde_json::from_value(record)?,
            rkey,
            validate,
            swap_cid: None,
        })
        .await?;

        let mut actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
        let backlink_conflicts: Vec<String> = match validate {
            Some(true) => {
                actor_store
                    .record
                    .get_backlink_conflicts(&write.uri, &write.record)
                    .await?
            }
            _ => Vec::new(),
        };

        // @TODO: Use ATUri
        let backlink_deletions: Vec<PreparedDelete> = backlink_conflicts
            .into_iter()
            .map(|uri| {
                let uri_without_prefix = uri.replace("at://", "");
                let parts = uri_without_prefix.split("/").collect::<Vec<&str>>();
                if let (Some(uri_hostname), Some(uri_collection), Some(uri_rkey)) =
                    (parts.get(0), parts.get(1), parts.get(2))
                {
                    Ok(prepare_delete(PrepareDeleteOpts {
                        did: uri_hostname.to_string(),
                        collection: uri_collection.to_string(),
                        rkey: uri_rkey.to_string(),
                        swap_cid: None,
                    }))
                } else {
                    bail!("Issue parsing backlink `{uri}`")
                }
            })
            .collect::<Result<Vec<PreparedDelete>>>()?;
        let mut writes: Vec<PreparedWrite> = vec![PreparedWrite::Create(write.clone())];
        for delete in backlink_deletions {
            writes.push(PreparedWrite::Delete(delete));
        }
        let commit = actor_store
            .process_writes(writes.clone(), swap_commit_cid)
            .await?;

        let mut lock = sequencer.sequencer.write().await;
        lock.sequence_commit(did.clone(), commit.clone(), writes)
            .await?;
        AccountManager::update_repo_root(did, commit.cid, commit.rev)?;

        Ok(CreateRecordOutput {
            uri: write.uri,
            cid: write.cid.to_string(),
        })
    } else {
        bail!("Could not find repo: `{repo}`")
    }
}

#[rocket::post(
    "/xrpc/com.atproto.repo.createRecord",
    format = "json",
    data = "<body>"
)]
pub async fn create_record(
    body: Json<CreateRecordInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<Json<CreateRecordOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    println!("@LOG: debug create_record {body:#?}");
    match inner_create_record(body, auth, sequencer, s3_config).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
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
