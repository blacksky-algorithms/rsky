use std::time::SystemTime;

pub mod errors;
pub mod oauth_provider;
pub mod oauth_routes;

pub fn now_as_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_secs()
}
