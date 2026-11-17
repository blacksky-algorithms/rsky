//! `com.atproto.memberspace.*` routes: member-list management for spaces
//! whose policy is `com.atproto.memberspace.policy` (spec §memberspace).

use crate::actor_store::space::{SpaceDefRow, SpacePolicy};
use crate::apis::ApiError;

pub mod list_members;
pub mod put_member;
pub mod remove_member;

/// Member-list methods only answer for a memberspace space: under a
/// different policy there is no member list to administer. The refusal is
/// `400 PolicyMismatch`, the name upstream `atproto-pds` gives it, so a client
/// can tell it from a malformed request.
pub fn require_memberspace(def: &SpaceDefRow) -> Result<(), ApiError> {
    if matches!(def.policy, SpacePolicy::Memberspace(_)) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "PolicyMismatch".to_string(),
            format!("space policy {} has no member list", def.policy.type_name()),
        ))
    }
}
