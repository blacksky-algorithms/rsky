//! failure stuff for when we encounter a new DID we'd like to sync

use crate::DidMethod;
use std::time::Duration;

/// TODO: i'm pretty sure we should, eventually, stop trying (return option?)
pub fn pending_resolve_backoff(method: DidMethod, attempts: u32) -> Duration {
    let seconds = match method {
        // plc errors are weird. directory down?
        //
        // TODO: verify never-heard-of vs deleted (which might get undone)
        DidMethod::Plc => match attempts {
            0 => 20,
            1 => 40,
            2 => 100,
            3..=6 => 600,
            _ => 3600, // for now we never stop trying, every 1h, forever
        },
        // did:web errors are less weird! servers go down all the time!
        //
        // backoff schedule here is pretty arbitrary
        DidMethod::Web => match attempts {
            0..=1 => 60,
            2..=4 => 600,
            5..=10 => 3600,
            _ => 86_400, // once per day forevermore
        },
    };
    Duration::from_secs(seconds)
}
