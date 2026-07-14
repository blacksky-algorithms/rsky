use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::SharedIdResolver;
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rocket::State;
use rsky_identity::types::DidDocument;
use rsky_lexicon::com::atproto::repo::DescribeRepoOutput;
use rsky_syntax::handle::INVALID_HANDLE;

async fn inner_describe_repo(
    repo: String,
    id_resolver: &State<SharedIdResolver>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<DescribeRepoOutput> {
    let account = account_manager.get_account(&repo, None).await?;
    match account {
        None => bail!("Cound not find user: `{repo}`"),
        Some(account) => {
            let mut lock = id_resolver.id_resolver.write().await;
            let did_doc: DidDocument = match lock.did.ensure_resolve(&account.did, None).await {
                Err(err) => bail!("Could not resolve DID: `{err}`"),
                Ok(res) => res,
            };
            let handle = rsky_common::get_handle(&did_doc);
            let handle_is_correct = handle == account.handle;

            let mut actor_store = actor_store
                .read(
                    account.did.clone(),
                    blobstore_factory.blobstore(account.did.clone()),
                )
                .await?;
            let collections = actor_store.record.list_collections().await?;

            Ok(DescribeRepoOutput {
                handle: account.handle.unwrap_or(INVALID_HANDLE.to_string()),
                did: account.did,
                did_doc: serde_json::to_value(did_doc)?,
                collections,
                handle_is_correct,
            })
        }
    }
}

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.repo.describeRepo?<repo>")]
pub async fn describe_repo(
    repo: String,
    id_resolver: &State<SharedIdResolver>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<Json<DescribeRepoOutput>, ApiError> {
    match inner_describe_repo(
        repo,
        id_resolver,
        blobstore_factory,
        actor_store,
        account_manager,
    )
    .await
    {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("{error:?}");
            Err(ApiError::RuntimeError)
        }
    }
}
