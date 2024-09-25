use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::types::PreparedWrite;
use crate::repo::{
    prepare_create, prepare_delete, prepare_update, ActorStore, PrepareCreateOpts,
    PrepareDeleteOpts, PrepareUpdateOpts,
};
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use futures::stream::{self, StreamExt};
use libipld::Cid;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{ApplyWritesInput, ApplyWritesInputRefWrite};
use std::str::FromStr;

async fn inner_apply_writes(
    body: Json<ApplyWritesInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<()> {
    let tx: ApplyWritesInput = body.into_inner();
    let ApplyWritesInput {
        repo,
        validate,
        swap_commit,
        ..
    } = tx;
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
        let did: &String = &did;
        if tx.writes.len() > 200 {
            bail!("Too many writes. Max: 200")
        }

        let writes: Vec<PreparedWrite> = stream::iter(tx.writes)
            .then(|write| async move {
                Ok::<PreparedWrite, anyhow::Error>(match write {
                    ApplyWritesInputRefWrite::Create(write) => PreparedWrite::Create(
                        prepare_create(PrepareCreateOpts {
                            did: did.clone(),
                            collection: write.collection,
                            rkey: write.rkey,
                            swap_cid: None,
                            record: serde_json::from_value(write.value)?,
                            validate,
                        })
                        .await?,
                    ),
                    ApplyWritesInputRefWrite::Update(write) => PreparedWrite::Update(
                        prepare_update(PrepareUpdateOpts {
                            did: did.clone(),
                            collection: write.collection,
                            rkey: write.rkey,
                            swap_cid: None,
                            record: serde_json::from_value(write.value)?,
                            validate,
                        })
                        .await?,
                    ),
                    ApplyWritesInputRefWrite::Delete(write) => {
                        PreparedWrite::Delete(prepare_delete(PrepareDeleteOpts {
                            did: did.clone(),
                            collection: write.collection,
                            rkey: write.rkey,
                            swap_cid: None,
                        }))
                    }
                })
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<PreparedWrite>, _>>()?;

        let swap_commit_cid = match swap_commit {
            Some(swap_commit) => Some(Cid::from_str(&swap_commit)?),
            None => None,
        };

        let mut actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));

        let commit = actor_store
            .process_writes(writes.clone(), swap_commit_cid)
            .await?;

        let mut lock = sequencer.sequencer.write().await;
        lock.sequence_commit(did.clone(), commit.clone(), writes)
            .await?;
        AccountManager::update_repo_root(did.to_string(), commit.cid, commit.rev)?;
        Ok(())
    } else {
        bail!("Could not find repo: `{repo}`")
    }
}

#[rocket::post("/xrpc/com.atproto.repo.applyWrites", format = "json", data = "<body>")]
pub async fn apply_writes(
    body: Json<ApplyWritesInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    println!("@LOG: debug apply_writes {body:#?}");
    match inner_apply_writes(body, auth, sequencer, s3_config).await {
        Ok(()) => Ok(()),
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
