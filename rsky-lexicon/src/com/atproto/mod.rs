pub mod admin;
pub mod identity;
pub mod label;
pub mod managedspace;
pub mod memberspace;
pub mod moderation;
pub mod personalspace;
pub mod repo;
pub mod server;
#[deprecated(
    since = "0.13.0",
    note = "Use com.atproto.space.*, memberspace.*, managedspace.*, or personalspace.* instead. Maintained for backward compatibility."
)]
pub mod simplespace;
pub mod space;
pub mod sync;
pub mod temp;
