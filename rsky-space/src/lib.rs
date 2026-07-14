//! `rsky-space` — atproto permissioned-data (spaces) primitives.
//!
//! Permissioned data is a **parallel protocol** to public atproto broadcast: a
//! user's records for a shared context ("space") live in a per-user
//! *permissioned repo* on their own PDS, summarized by a deniable LtHash
//! commit, and gated by credentials issued by a space authority. This crate is
//! the Rust analog of the upstream `@atproto/space` package — the shared
//! primitives consumed by both a space **authority/host** and a **syncer**:
//!
//! - [`car`] — permissioned repo CAR serialization and streaming validation.
//! - [`lthash`] — the homomorphic set-hash commitment over a repo's records.
//! - [`commit`] — the domain-separated commit context and deniable
//!   signature/MAC verification.
//! - [`credential`] — delegation tokens, space credentials, and client
//!   attestations (the JWT envelope + verification).
//! - [`space_id`] — space and permissioned-record `at://.../space/...`
//!   addressing.
//! - [`types`] — signed commits, repo ops, and writer-set references.
//!
//! PDS-side hosting of permissioned repos is deferred to upstream
//! (`bluesky-social/atproto` PR #5187); this crate builds and is tested against
//! spec vectors independently of that.

pub mod car;
pub mod commit;
pub mod credential;
pub mod error;
pub mod lthash;
pub mod space_id;
pub mod types;

pub use car::{repo_car_bytes, write_repo_car, RepoCarValidator};
pub use error::{Result, SpaceError};
pub use lthash::LtHash;
pub use space_id::{is_space_uri, RecordId, SpaceId};
pub use types::{RepoOp, RepoRef, SignedCommit};
