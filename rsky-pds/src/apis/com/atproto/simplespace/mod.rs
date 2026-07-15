//! `com.atproto.simplespace.*` routes: the space-management implementation
//! every PDS must support. Spaces are anchored on the caller's own DID and
//! governed by a member list (or the `public`/`managing-app` policies).
//!
//! All methods require an OAuth credential with the relevant `manage` scope.
//! Sessions without a scope carrier evaluate through the same
//! `crate::space_auth::session_permits` seam as the space routes (see its
//! module docs).

use crate::actor_store::space::{SpaceDefRow, SpaceStoreError};
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::{
    APP_ACCESS_ALLOW_LIST, APP_ACCESS_OPEN, POLICY_MANAGING_APP, POLICY_MEMBER_LIST, POLICY_PUBLIC,
};
use crate::apis::ApiError;
use crate::auth_verifier::Credentials;
use crate::space_auth::session_permits;
use crate::space_scope::{ManageOp, SpaceRequest};
use rsky_lexicon::com::atproto::simplespace::{
    AppAccess as LexAppAccess, Config, Policy as LexPolicy,
};
use rsky_space::space_id::SpaceId;

pub mod add_member;
pub mod create_space;
pub mod delete_space;
pub mod list_members;
pub mod remove_member;
pub mod update_space;

pub fn space_error(error: anyhow::Error) -> ApiError {
    match error.downcast_ref::<SpaceStoreError>() {
        Some(SpaceStoreError::RecordExists(_)) => ApiError::BadRequest(
            "SpaceExists".to_string(),
            "a space with this type and key already exists".to_string(),
        ),
        _ => crate::apis::com::atproto::space::space_error(error),
    }
}

/// simplespace spaces are anchored on the caller's own DID: the caller must be
/// the authority, and their session must carry the manage capability.
pub fn require_manage(
    credentials: &Credentials,
    did: &str,
    space: &SpaceId,
    op: ManageOp,
) -> Result<(), ApiError> {
    if space.authority != did {
        return Err(ApiError::AuthRequiredError(
            "only the space authority may manage a simplespace".to_string(),
        ));
    }
    if !session_permits(credentials, did, space, &SpaceRequest::Manage(op)) {
        return Err(ApiError::AuthRequiredError(
            "session does not cover managing this space".to_string(),
        ));
    }
    Ok(())
}

/// Fold a lexicon config into a definition row, validating the combination.
pub fn merge_config(mut def: SpaceDefRow, config: &Config) -> Result<SpaceDefRow, ApiError> {
    if let Some(ref policy) = config.policy {
        def.policy = match policy {
            LexPolicy::Public => POLICY_PUBLIC.to_string(),
            LexPolicy::MemberList => POLICY_MEMBER_LIST.to_string(),
            LexPolicy::ManagingApp => POLICY_MANAGING_APP.to_string(),
        };
    }
    if let Some(ref app_access) = config.app_access {
        match app_access {
            LexAppAccess::Open(_) => {
                def.app_access = APP_ACCESS_OPEN.to_string();
                def.allowed_clients = None;
            }
            LexAppAccess::AllowList(list) => {
                def.app_access = APP_ACCESS_ALLOW_LIST.to_string();
                def.allowed_clients = Some(list.allowed.clone());
            }
        }
    }
    if let Some(ref managing_app) = config.managing_app {
        if !managing_app.contains('#') || !managing_app.starts_with("did:") {
            return Err(ApiError::InvalidRequest(format!(
                "managingApp must be a service identifier (did#fragment): {managing_app}"
            )));
        }
        def.managing_app = Some(managing_app.clone());
    }
    if def.policy == POLICY_MANAGING_APP && def.managing_app.is_none() {
        return Err(ApiError::InvalidRequest(
            "managing-app policy requires a managingApp".to_string(),
        ));
    }
    Ok(def)
}

/// Resolve the caller's keypair for signed notifications.
pub async fn actor_keypair(
    actor_store: &ActorStore,
    did: &str,
) -> Result<secp256k1::Keypair, ApiError> {
    actor_store
        .keypair(did)
        .await
        .map_err(crate::apis::com::atproto::space::internal_error(
            "missing actor keypair",
        ))
}
