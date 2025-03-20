use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::db::DbConn;
use crate::repo::prepare::{prepare_create, prepare_update, PrepareCreateOpts, PrepareUpdateOpts};
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use libipld::Cid;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{PutRecordInput, PutRecordOutput};
use rsky_repo::types::{CommitData, CommitDataWithOps, PreparedWrite};
use rsky_syntax::aturi::AtUri;
use std::str::FromStr;

#[tracing::instrument(skip_all)]
async fn inner_put_record(
    body: Json<PutRecordInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<PutRecordOutput> {
    let PutRecordInput {
        repo,
        collection,
        rkey,
        validate,
        record,
        swap_record,
        swap_commit,
    } = body.into_inner();
    let account = account_manager
        .get_account(
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
        let uri = AtUri::make(did.clone(), Some(collection.clone()), Some(rkey.clone()))?;
        let swap_commit_cid = match swap_commit {
            Some(swap_commit) => Some(Cid::from_str(&swap_commit)?),
            None => None,
        };
        let swap_record_cid = match swap_record {
            Some(swap_record) => Some(Cid::from_str(&swap_record)?),
            None => None,
        };
        let (commit, write): (Option<CommitDataWithOps>, PreparedWrite) = {
            let mut actor_store =
                ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);

            let current = actor_store
                .record
                .get_record(&uri, None, Some(true))
                .await?;
            tracing::debug!("@LOG: debug inner_put_record, current: {current:?}");
            let write: PreparedWrite = if current.is_some() {
                PreparedWrite::Update(
                    prepare_update(PrepareUpdateOpts {
                        did: did.clone(),
                        collection,
                        rkey,
                        swap_cid: swap_record_cid,
                        record: serde_json::from_value(record)?,
                        validate,
                    })
                    .await?,
                )
            } else {
                PreparedWrite::Create(
                    prepare_create(PrepareCreateOpts {
                        did: did.clone(),
                        collection,
                        rkey: Some(rkey),
                        swap_cid: swap_record_cid,
                        record: serde_json::from_value(record)?,
                        validate,
                    })
                    .await?,
                )
            };

            match current {
                Some(current) if current.cid == write.cid().unwrap().to_string() => (None, write),
                _ => {
                    let commit = actor_store
                        .process_writes(vec![write.clone()], swap_commit_cid)
                        .await?;
                    (Some(commit), write)
                }
            }
        };

        if let Some(commit) = commit {
            let mut lock = sequencer.sequencer.write().await;
            lock.sequence_commit(did.clone(), commit.clone())
                .await?;
            account_manager
                .update_repo_root(did, commit.commit_data.cid, commit.commit_data.rev)
                .await?;
        }
        Ok(PutRecordOutput {
            uri: write.uri().to_string(),
            cid: write.cid().unwrap().to_string(),
        })
    } else {
        bail!("Could not find repo: `{repo}`")
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.repo.putRecord", format = "json", data = "<body>")]
pub async fn put_record(
    body: Json<PutRecordInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<Json<PutRecordOutput>, ApiError> {
    tracing::debug!("@LOG: debug put_record {body:#?}");
    match inner_put_record(body, auth, sequencer, s3_config, db, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
