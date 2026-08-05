//! Storage abstraction for atproto PDS blobs.
//!
//! This crate owns the [`BlobStore`] trait, the [`BlobNotFoundError`] error
//! type, and a reusable [`MemoryBlobStore`] test double. It deliberately
//! avoids pulling in heavyweight dependencies (AWS SDK, Rocket, rusqlite) so
//! external consumers can implement the trait against any backend.

pub mod blobstore;

pub use blobstore::{
    generation_name, unavailable, BlobNotFoundError, BlobStore, BlobUnavailableError,
    BoxedBlobStream, DeleteError, MemoryBlobStore, ObjectKind,
};
