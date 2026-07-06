//! `rsky-space-host` — the space authority/host for permissioned data.
//!
//! A space authority answers for a space as a whole: it issues [space
//! credentials](authority) to authorized readers, describes the space
//! (`getSpace`), enumerates the writer set (`listRepos`), and routes write
//! notifications. It is a dedicated service — NOT the appview — because
//! permissioned data is a parallel protocol.
//!
//! For the Blacksky community it runs under the `managing-app` policy: the
//! per-user access decision is the `blacksky-beta` [membership](membership)
//! list, read directly by the host. Per-app access is gated by
//! [`appaccess::AppAccess`].
//!
//! This first pass implements the credential-mint core (verify delegation →
//! managing-app + appAccess checks → mint, all tested). The HTTP surface
//! (`getSpace`/`getSpaceCredential`/`listRepos`/notification routing) and the
//! Postgres membership reader are the next step; the writer-set/notification
//! state depends on the upstream `com.atproto.space.*` shapes (PR #5187).

pub mod appaccess;
pub mod authority;
pub mod config;
pub mod error;
pub mod membership;
pub mod signing;

pub use authority::{Authority, KeyResolver, SpaceConfig};
pub use error::{HostError, Result};
