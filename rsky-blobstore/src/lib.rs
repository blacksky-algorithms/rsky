//! Storage abstraction for atproto PDS blobs.
//!
//! This crate owns the [`BlobStore`] trait, its error types, a reusable
//! [`MemoryBlobStore`] test double, and optional backend implementations
//! under the [`backends`] module. It deliberately avoids pulling in
//! heavyweight dependencies (AWS SDK, Rocket, rusqlite) unless the
//! corresponding backend feature is enabled.

pub mod backends;
pub mod blobstore;

pub use blobstore::{
    generation_name, unavailable, BlobNotFoundError, BlobStore, BlobUnavailableError,
    BoxedBlobStream, DeleteError, MemoryBlobStore, ObjectKind,
};

#[cfg(feature = "disk")]
pub use backends::disk::DiskBlobStore;

#[cfg(feature = "opendal")]
pub use backends::opendal::{build_operator, new_blobstore, OpendalBlobStore};
