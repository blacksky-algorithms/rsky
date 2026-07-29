//! `rsky-space-host` — the space authority/host for permissioned data.
//!
//! A space authority answers for a space as a whole: it issues [space
//! credentials](authority) to authorized readers, describes the space
//! (`getSpace`), enumerates the writer set (`listRepos`), and routes write
//! notifications. It is a dedicated service — NOT the appview — because
//! permissioned data is a parallel protocol.
//!
//! Authorization is two-axis (spec §Access control): the per-user decision is a
//! [`policy::Policy`] (`public`, `member-list`, or `managing-app`, where the
//! host defers to the space's managing app via `checkUserAccess` and never
//! reads the membership decision itself), and the per-app decision is
//! [`appaccess::AppAccess`], enforced against a verified
//! [client attestation](attestation).
//!
//! The XRPC surface ([`http`]) serves `getSpace`, `getSpaceCredential`,
//! `listRepos`, `registerNotify`, and `notifyWrite` using the
//! `com.atproto.space.*` DTOs from rsky-lexicon, backed by in-memory or SQLite
//! [stores](store).

pub mod appaccess;
pub mod attestation;
pub mod authority;
pub mod config;
pub mod error;
pub mod http;
pub mod keys;
pub mod managing_app;
pub mod membership;
pub mod notify;
pub mod policy;
pub mod service_jwt;
pub mod signing;
pub mod store;

pub use authority::{Authority, KeyResolver};
pub use error::{HostError, Result};
pub use policy::Policy;
