use crate::account_manager::helpers::account::{ActorAccount, AvailabilityFlags};
use crate::account_manager::AccountManager;
use anyhow::{bail, Result};

pub async fn assert_repo_availability(
    did: &String,
    is_admin_of_self: bool,
) -> Result<ActorAccount> {
    let account = AccountManager::get_account(
        did,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: Some(true),
        }),
    )
    .await?;
    match account {
        None => bail!("RepoNotFound: Could not find repo for DID: {did}"),
        Some(account) => {
            if is_admin_of_self {
                return Ok(account);
            }
            if account.takedown_ref.is_some() {
                bail!("RepoTakendown: Repo has been takendown: {did}");
            }
            if account.deactivated_at.is_some() {
                bail!("RepoDeactivated: Repo has been deactivated: {did}");
            }
            Ok(account)
        }
    }
}

pub mod apply_writes;
pub mod create_record;
pub mod delete_record;
pub mod describe_repo;
pub mod get_record;
pub mod import_repo;
pub mod list_missing_blobs;
pub mod list_records;
pub mod put_record;
pub mod upload_blob;
