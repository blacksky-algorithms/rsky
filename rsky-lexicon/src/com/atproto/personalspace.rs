//! `com.atproto.personalspace.*` types: the baseline policy for a space only
//! its authority can read and write.

/// `com.atproto.personalspace.policy`: no configuration fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Policy {}
