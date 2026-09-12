use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::publication;
use crate::rate_limits::{Caller, RateLimits};
use crate::repo::prepare::{
    prepare_create, prepare_delete, prepare_update, PrepareCreateOpts, PrepareDeleteOpts,
    PrepareUpdateOpts,
};
use crate::SharedSequencer;
use anyhow::{bail, Result};
use futures::stream::{self, StreamExt};
use lexicon_cid::Cid;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::{
    ApplyWritesInput, ApplyWritesInputRefWrite, ApplyWritesOutput, ApplyWritesOutputResult,
    ApplyWritesResultCreate, ApplyWritesResultDelete, ApplyWritesResultUpdate, CommitMeta,
};
use rsky_repo::types::PreparedWrite;
use std::str::FromStr;

async fn inner_apply_writes(
    body: Json<ApplyWritesInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<ApplyWritesOutput> {
    let tx: ApplyWritesInput = body.into_inner();
    let ApplyWritesInput {
        repo,
        validate,
        swap_commit,
        ..
    } = tx;
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
        let did: &String = &did;

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
                        })?)
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

        let mut actor_txn = actor_store
            .transact(did.clone(), blobstore_factory.blobstore(did.clone()))
            .await?;

        let commit = actor_txn
            .process_writes(writes.clone(), swap_commit_cid)
            .await?;

        let commit_cid = commit.commit_data.cid.to_string();
        let commit_rev = commit.commit_data.rev.clone();
        publication::publish_pending(actor_store, sequencer, did, None).await?;
        account_manager
            .update_repo_root(
                did.to_string(),
                commit.commit_data.cid,
                commit.commit_data.rev,
            )
            .await?;
        // The lexicon declares a JSON object output; returning an empty body
        // instead makes a client that requires JSON treat a successful write as
        // failed and retry it, duplicating records.
        let results = writes
            .iter()
            .map(|write| match write {
                PreparedWrite::Create(w) => {
                    ApplyWritesOutputResult::Create(ApplyWritesResultCreate {
                        uri: w.uri.clone(),
                        cid: w.cid.to_string(),
                        validation_status: None,
                    })
                }
                PreparedWrite::Update(w) => {
                    ApplyWritesOutputResult::Update(ApplyWritesResultUpdate {
                        uri: w.uri.clone(),
                        cid: w.cid.to_string(),
                        validation_status: None,
                    })
                }
                PreparedWrite::Delete(_) => {
                    ApplyWritesOutputResult::Delete(ApplyWritesResultDelete {})
                }
            })
            .collect();
        Ok(ApplyWritesOutput {
            commit: Some(CommitMeta {
                cid: commit_cid,
                rev: commit_rev,
            }),
            results: Some(results),
        })
    } else {
        bail!("Could not find repo: `{repo}`")
    }
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.repo.applyWrites", format = "json", data = "<body>")]
pub async fn apply_writes(
    body: Json<ApplyWritesInput>,
    auth: AccessStandardIncludeChecks,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<Json<ApplyWritesOutput>, ApiError> {
    limits.consume_all(
        &crate::rate_limits::REPO_WRITES,
        auth.access
            .credentials
            .as_ref()
            .and_then(|credentials| credentials.did.as_deref())
            .unwrap_or_default(),
        body.writes
            .iter()
            .map(|write| match write {
                ApplyWritesInputRefWrite::Create(_) => crate::rate_limits::CREATE_POINTS,
                ApplyWritesInputRefWrite::Update(_) => crate::rate_limits::UPDATE_POINTS,
                ApplyWritesInputRefWrite::Delete(_) => crate::rate_limits::DELETE_POINTS,
            })
            .sum(),
        caller.bypass,
    )?;
    tracing::debug!("@LOG: debug apply_writes {body:#?}");
    for write in &body.writes {
        let (collection, action) = match write {
            ApplyWritesInputRefWrite::Create(w) => {
                (&w.collection, crate::oauth_scope::RepoAction::Create)
            }
            ApplyWritesInputRefWrite::Update(w) => {
                (&w.collection, crate::oauth_scope::RepoAction::Update)
            }
            ApplyWritesInputRefWrite::Delete(w) => {
                (&w.collection, crate::oauth_scope::RepoAction::Delete)
            }
        };
        crate::apis::assert_repo_scope(&auth.access.credentials, collection, action)?;
    }
    match inner_apply_writes(
        body,
        auth,
        sequencer,
        blobstore_factory,
        actor_store,
        account_manager,
    )
    .await
    {
        Ok(output) => Ok(Json(output)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::from(error))
        }
    }
}
