use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::db::DbConn;
use crate::repo::prepare::{prepare_delete, PrepareDeleteOpts};
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use libipld::Cid;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::DeleteRecordInput;
use rsky_repo::types::PreparedWrite;
use rsky_syntax::aturi::AtUri;
use std::str::FromStr;

async fn inner_delete_record(
    body: Json<DeleteRecordInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<()> {
    let DeleteRecordInput {
        repo,
        collection,
        rkey,
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
    match account {
        None => bail!("Could not find repo: `{repo}`"),
        Some(account) if account.deactivated_at.is_some() => bail!("Account is deactivated"),
        Some(account) => {
            let did = account.did;
            if did != auth.access.credentials.unwrap().did.unwrap() {
                bail!("AuthRequiredError")
            }

            let swap_commit_cid = match swap_commit {
                Some(swap_commit) => Some(Cid::from_str(&swap_commit)?),
                None => None,
            };
            let swap_record_cid = match swap_record {
                Some(swap_record) => Some(Cid::from_str(&swap_record)?),
                None => None,
            };

            let write = prepare_delete(PrepareDeleteOpts {
                did: did.clone(),
                collection,
                rkey,
                swap_cid: swap_record_cid,
            })?;
            let mut actor_store =
                ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
            let write_at_uri: AtUri = write.uri.clone().try_into()?;
            let record = actor_store
                .record
                .get_record(&write_at_uri, None, Some(true))
                .await?;
            let commit = match record {
                None => return Ok(()), // No-op if record already doesn't exist
                Some(_) => {
                    actor_store
                        .process_writes(vec![PreparedWrite::Delete(write.clone())], swap_commit_cid)
                        .await?
                }
            };

            let mut lock = sequencer.sequencer.write().await;
            lock.sequence_commit(
                did.clone(),
                commit.clone(),
                vec![PreparedWrite::Delete(write)],
            )
            .await?;
            account_manager
                .update_repo_root(did, commit.cid, commit.rev)
                .await?;

            Ok(())
        }
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.repo.deleteRecord",
    format = "json",
    data = "<body>"
)]
pub async fn delete_record(
    body: Json<DeleteRecordInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_delete_record(body, auth, sequencer, s3_config, db, account_manager).await {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
