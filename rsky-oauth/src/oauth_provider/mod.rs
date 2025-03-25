use std::time::SystemTime;

pub mod access_token;
pub mod account;
pub mod client;
pub mod constants;
pub mod device;
pub mod dpop;
pub mod errors;
pub mod lib;
pub mod metadata;
pub mod oauth_hooks;
pub mod oauth_provider;
mod oauth_store;
pub mod oauth_verifier;
pub mod oidc;
pub mod output;
pub mod replay;
pub mod request;
pub mod routes;
pub mod signer;
pub mod token;

pub fn now_as_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_secs()
}
