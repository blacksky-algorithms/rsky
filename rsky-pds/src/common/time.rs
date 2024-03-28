use std::time::SystemTime;
use chrono::DateTime;
use chrono::offset::Utc as UtcOffset;

pub const SECOND: i32 = 1000;
pub const MINUTE: i32 = SECOND * 60;

pub fn less_than_ago_ms(time: DateTime<UtcOffset>, range: i32) -> bool {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_micros() as usize;
    now < (time.timestamp() as usize + range as usize)
}