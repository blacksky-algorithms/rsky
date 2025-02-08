use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::db::DbConn;
use crate::repo::prepare::{prepare_create, prepare_delete, PrepareCreateOpts, PrepareDeleteOpts};
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use libipld::Cid;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{CreateRecordInput, CreateRecordOutput};
use rsky_repo::types::{PreparedDelete, PreparedWrite};
use rsky_syntax::aturi::AtUri;
use std::str::FromStr;

async fn inner_create_record(
    body: Json<CreateRecordInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
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
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
        let backlink_conflicts: Vec<AtUri> = match validate {
            Some(true) => {
                let write_at_uri: AtUri = write.uri.clone().try_into()?;
                actor_store
                    .record
                    .get_backlink_conflicts(&write_at_uri, &write.record)
                    .await?
            }
            _ => Vec::new(),
        };

        let backlink_deletions: Vec<PreparedDelete> = backlink_conflicts
            .iter()
            .map(|at_uri| {
                prepare_delete(PrepareDeleteOpts {
                    did: at_uri.get_hostname().to_string(),
                    collection: at_uri.get_collection(),
                    rkey: at_uri.get_rkey(),
                    swap_cid: None,
                })
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
            uri: write.uri.clone(),
            cid: write.cid.to_string(),
        })
    } else {
        bail!("Could not find repo: `{repo}`")
    }
}

#[tracing::instrument(skip_all)]
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
    db: DbConn,
) -> Result<Json<CreateRecordOutput>, ApiError> {
    tracing::debug!("@LOG: debug create_record {body:#?}");
    match inner_create_record(body, auth, sequencer, s3_config, db).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
