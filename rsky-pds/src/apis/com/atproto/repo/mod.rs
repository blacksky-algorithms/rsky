use crate::account_manager::helpers::account::{ActorAccount, AvailabilityFlags};
use crate::account_manager::AccountManager;
use anyhow::Result;

/// Why a repository cannot be served to the caller. Rendered with the
/// reference error names so sync consumers see the same responses.
#[derive(Debug, thiserror::Error)]
pub enum RepoUnavailable {
    #[error("Could not find repo for DID: {0}")]
    NotFound(String),
    #[error("Repo has been takendown: {0}")]
    Takendown(String),
    #[error("Repo has been deactivated: {0}")]
    Deactivated(String),
}

pub async fn assert_repo_availability(
    did: &str,
    is_admin_of_self: bool,
    account_manager: &AccountManager,
) -> Result<ActorAccount> {
    let account = account_manager
        .get_account(
            did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    match account {
        None => Err(RepoUnavailable::NotFound(did.to_owned()).into()),
        Some(account) => {
            if is_admin_of_self {
                return Ok(account);
            }
            if account.takedown_ref.is_some() {
                return Err(RepoUnavailable::Takendown(did.to_owned()).into());
            }
            if account.deactivated_at.is_some() {
                return Err(RepoUnavailable::Deactivated(did.to_owned()).into());
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
