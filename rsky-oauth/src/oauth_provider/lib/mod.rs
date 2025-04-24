use std::time::SystemTime;

pub mod html;
pub mod http;
pub mod util;

pub fn current_epoch() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in millis since UNIX epoch")
        .as_millis() as u64
}
