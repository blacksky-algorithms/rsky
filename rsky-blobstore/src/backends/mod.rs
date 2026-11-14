//! Optional blobstore backend implementations.

#[cfg(feature = "disk")]
pub mod disk;

#[cfg(feature = "opendal")]
pub mod opendal;
