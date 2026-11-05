//! Backward compatibility layer for legacy simplespace API.
//!
//! All endpoints in this module are supported indefinitely for backward
//! compatibility. They forward to new spec-compliant implementations after
//! translating between old and new data formats.
//!
//! New applications should use the newer API namespaces:
//! - com.atproto.space.*
//! - com.atproto.memberspace.*
//! - com.atproto.managedspace.*
//! - com.atproto.personalspace.*
//!
//! NOTE: getSpace is NOT included - didn't exist in old API
//! NOTE: checkUserAccess is NOT included - it's served by managing apps, not the PDS

pub mod adapters;
pub mod add_member;
pub mod create_space;
pub mod delete_space;
pub mod list_members;
pub mod remove_member;
pub mod update_space;

pub use add_member::simplespace_add_member;
pub use create_space::simplespace_create_space;
pub use delete_space::simplespace_delete_space;
pub use list_members::simplespace_list_members;
pub use remove_member::simplespace_remove_member;
pub use update_space::simplespace_update_space;
