//! Process-memory and queue-store gauges, so resident size is observable from
//! `/metrics` instead of `ssh` + `ps`.
//!
//! The prometheus `process` feature already exports
//! `process_resident_memory_bytes` on Linux. These gauges add what it lacks:
//! the peak (`VmHWM`), which is what the kernel OOM killer actually judged,
//! and the Fjall write-buffer size, which is the one component of RSS the
//! daemon controls directly through `FJALL_MEMTABLE_MB` /
//! `FJALL_WRITE_BUFFER_SIZE_GB`.
//!
//! Everything here is sampled at scrape time from `/proc/self/status` and the
//! keyspace's own counters; nothing runs on a timer.

use std::sync::{Arc, LazyLock, OnceLock, Weak};

use prometheus::{IntGauge, register_int_gauge};

use crate::storage::Storage;

pub static WINTERMUTE_RSS_BYTES: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "wintermute_rss_bytes",
        "Resident set size of this process (VmRSS); 0 where /proc is unavailable"
    )
    .unwrap_or_else(|e| panic!("wintermute_rss_bytes: {e}"))
});

pub static WINTERMUTE_RSS_PEAK_BYTES: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "wintermute_rss_peak_bytes",
        "Peak resident set size since start (VmHWM); 0 where /proc is unavailable"
    )
    .unwrap_or_else(|e| panic!("wintermute_rss_peak_bytes: {e}"))
});

pub static WINTERMUTE_FJALL_WRITE_BUFFER_BYTES: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "wintermute_fjall_write_buffer_bytes",
        "Bytes currently held in Fjall memtables across all queue partitions"
    )
    .unwrap_or_else(|e| panic!("wintermute_fjall_write_buffer_bytes: {e}"))
});

pub static WINTERMUTE_FJALL_JOURNAL_COUNT: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "wintermute_fjall_journal_count",
        "Fjall journal files open; more than a handful means flushes are falling behind"
    )
    .unwrap_or_else(|e| panic!("wintermute_fjall_journal_count: {e}"))
});

pub static WINTERMUTE_FJALL_DISK_BYTES: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "wintermute_fjall_disk_bytes",
        "On-disk size of the Fjall queue store"
    )
    .unwrap_or_else(|e| panic!("wintermute_fjall_disk_bytes: {e}"))
});

/// The daemon's queue store, held weakly so this registry never extends its
/// lifetime past shutdown.
static STORAGE: OnceLock<Weak<Storage>> = OnceLock::new();

/// Register the queue store whose memtable and journal figures should be
/// exported. Only the first registration wins; the daemon has one store.
pub fn register_storage(storage: &Arc<Storage>) {
    drop(STORAGE.set(Arc::downgrade(storage)));
}

/// Resident and peak-resident sizes in bytes, from `/proc/self/status`.
#[cfg(target_os = "linux")]
fn read_rss() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_status(&status)
}

#[cfg(not(target_os = "linux"))]
const fn read_rss() -> Option<(u64, u64)> {
    None
}

/// Pull `VmRSS` and `VmHWM` (both reported in kB) out of a `/proc/<pid>/status`
/// body. Returns `None` if either line is missing or malformed.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_status(status: &str) -> Option<(u64, u64)> {
    let mut rss = None;
    let mut peak = None;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            rss = parse_kb(rest);
        } else if let Some(rest) = line.strip_prefix("VmHWM:") {
            peak = parse_kb(rest);
        }
    }
    Some((rss?, peak?))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_kb(field: &str) -> Option<u64> {
    let mut parts = field.split_whitespace();
    let n: u64 = parts.next()?.parse().ok()?;
    if parts.next() != Some("kB") {
        return None;
    }
    n.checked_mul(1024)
}

fn to_gauge(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Refresh every gauge in this module. Called from the `/metrics` encoder.
pub fn sample() {
    let (rss, peak) = read_rss().unwrap_or((0, 0));
    WINTERMUTE_RSS_BYTES.set(to_gauge(rss));
    WINTERMUTE_RSS_PEAK_BYTES.set(to_gauge(peak));

    if let Some(storage) = STORAGE.get().and_then(Weak::upgrade) {
        let stats = storage.keyspace_stats();
        WINTERMUTE_FJALL_WRITE_BUFFER_BYTES.set(to_gauge(stats.write_buffer_bytes));
        WINTERMUTE_FJALL_JOURNAL_COUNT.set(to_gauge(stats.journal_count));
        WINTERMUTE_FJALL_DISK_BYTES.set(to_gauge(stats.disk_bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Name:\twintermute\nVmPeak:\t  4417197465 kB\nVmSize:\t  4417197465 kB\n\
                          VmHWM:\t 9727288 kB\nVmRSS:\t 7761684 kB\nVmSwap:\t       0 kB\n";

    #[test]
    fn parses_rss_and_peak_in_bytes() {
        assert_eq!(
            parse_status(SAMPLE),
            Some((7_761_684 * 1024, 9_727_288 * 1024))
        );
    }

    #[test]
    fn missing_line_yields_none() {
        assert_eq!(parse_status("VmRSS:\t 12 kB\n"), None);
        assert_eq!(parse_status(""), None);
    }

    #[test]
    fn rejects_unexpected_units() {
        assert_eq!(parse_kb("  12 MB"), None);
        assert_eq!(parse_kb("  12"), None);
        assert_eq!(parse_kb("  x kB"), None);
    }

    #[test]
    fn sample_never_panics_without_a_store() {
        sample();
        // On Linux the process has a resident size; elsewhere the gauge is 0.
        if cfg!(target_os = "linux") {
            assert!(WINTERMUTE_RSS_BYTES.get() > 0);
            assert!(WINTERMUTE_RSS_PEAK_BYTES.get() >= WINTERMUTE_RSS_BYTES.get());
        } else {
            assert_eq!(WINTERMUTE_RSS_BYTES.get(), 0);
        }
    }
}
