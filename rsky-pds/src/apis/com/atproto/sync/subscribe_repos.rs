use crate::common::env::env_int;
use crate::common::time::DAY;
use crate::common::RFC3339_VARIANT;
use crate::SharedSequencer;
use chrono::offset::Utc as UtcOffset;
use chrono::{DateTime, Duration};
use rocket::State;
use std::time::SystemTime;

fn get_backfill_limit(ms: usize) -> String {
    let system_time = SystemTime::now();
    let mut dt: DateTime<UtcOffset> = system_time.into();
    dt = dt - Duration::milliseconds(ms as i64);
    format!("{}", dt.format(RFC3339_VARIANT))
}

/// Repository event stream, aka Firehose endpoint. Outputs repo commits with diff data,
/// and identity update events, for all repositories on the current server. See the atproto
/// specifications for details around stream sequencing, repo versioning, CAR diff format, and more.
/// Public and does not require auth; implemented by PDS and Relay.
#[rocket::get("/xrpc/com.atproto.sync.subscribeRepos?<cursor>")]
pub async fn subscribe_repos(
    // The last known event seq number to backfill from.
    cursor: Option<i64>,
    _sequencer: &State<SharedSequencer>,
    ws: ws::WebSocket,
) -> ws::Stream!['static] {
    println!("@LOG DEBUG: request to com.atproto.sync.subscribeRepos; Cursor={cursor:?}");
    let repo_backfill_limit_ms = env_int("PDS_REPO_BACKFILL_LIMIT_MS").unwrap_or(DAY as usize);
    let _backfill_time = get_backfill_limit(repo_backfill_limit_ms);

    ws::Stream! { ws =>
        for await message in ws {
            yield message?;
        }
    }
}
