use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::config::AppConfig;
use crate::error::{Error, Result};

/// How often the supervisor checks the growing output file against the size
/// ceiling.
const OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Cap on the ffmpeg stderr we buffer for error messages.
const STDERR_CAPTURE_LIMIT: u64 = 64 * 1024;

/// Resource ceilings applied to every ffmpeg invocation.
///
/// Transcoding is the only place this service hands an attacker-supplied file
/// to a CPU- and memory-hungry subprocess, so each conversion runs under a
/// wall-clock deadline, an output-size ceiling, a thread cap, and a global
/// concurrency limit. Cloning is cheap -- the semaphore is shared.
#[derive(Clone)]
pub struct Limits {
    timeout: Duration,
    queue_timeout: Duration,
    max_output_bytes: u64,
    threads: u32,
    slots: Arc<Semaphore>,
}

impl Limits {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            timeout: Duration::from_secs(config.transcode_timeout_secs),
            queue_timeout: Duration::from_secs(config.transcode_queue_timeout_secs),
            max_output_bytes: config.transcode_max_output_bytes,
            threads: config.transcode_threads,
            slots: Arc::new(Semaphore::new(config.transcode_max_concurrent)),
        }
    }

    /// Slots not currently held by a running conversion.
    pub fn available_slots(&self) -> usize {
        self.slots.available_permits()
    }
}

/// True when the payload is an animated/static GIF (magic bytes GIF87a/GIF89a).
pub fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
}

/// True when the payload is a QuickTime container.
///
/// This is what iPhone camera captures and screen recordings ship as -- H.264
/// and AAC inside a `.mov`. The PDS sniffs blob bytes instead of trusting the
/// content-type we send, so uploading these verbatim yields a blob tagged
/// `video/quicktime`, and `app.bsky.embed.video` -- which accepts only
/// `video/mp4` -- then rejects the record.
///
/// The predicate deliberately mirrors what the PDS sniffers treat as
/// QuickTime, since anything they tag `video/quicktime` fails lexicon
/// validation: an `ftyp` box with the `qt  ` brand (what iOS writes), or a
/// leading `moov`/`mdat`/`free`/`wide` box for older MOVs that carry no `ftyp`
/// at all. Verified against `file-type` 16.x (TypeScript PDS) and the `infer`
/// crate (rsky-pds). ISO BMFF brands (`isom`, `mp42`, ...) already sniff as
/// `video/mp4` and are left alone.
pub fn is_quicktime_container(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    match &bytes[4..8] {
        b"ftyp" => &bytes[8..12] == b"qt  ",
        b"moov" | b"mdat" | b"free" | b"wide" => true,
        _ => false,
    }
}

/// Convert a GIF to a silent MP4 that Bunny Stream can transcode.
///
/// Bunny's encoder rejects GIF input outright, so GIF uploads are converted
/// here first. yuv420p and even dimensions are required for broad H.264
/// playback; faststart keeps the moov atom at the front for streaming.
pub async fn gif_to_mp4(limits: &Limits, bytes: &[u8]) -> Result<Vec<u8>> {
    let mp4 = run_ffmpeg(
        limits,
        "gif->mp4",
        "in.gif",
        "gif",
        bytes,
        &[
            "-movflags",
            "faststart",
            "-pix_fmt",
            "yuv420p",
            "-vf",
            "scale=trunc(iw/2)*2:trunc(ih/2)*2",
            "-an",
        ],
    )
    .await?;
    info!(
        "transcoded gif ({} bytes) to mp4 ({} bytes)",
        bytes.len(),
        mp4.len()
    );
    Ok(mp4)
}

/// Remux a QuickTime `.mov` to MP4 without re-encoding.
///
/// `-c copy` keeps the existing H.264/AAC streams and only rewrites the
/// container, so this is lossless and fast -- the cost is copying the bytes
/// once (typically under a second for 100MB). MOVs carrying codecs MP4 cannot
/// hold (ProRes, for instance) fail here with ffmpeg's own error instead of
/// producing a blob the PDS would tag `video/quicktime`.
pub async fn mov_to_mp4(limits: &Limits, bytes: &[u8]) -> Result<Vec<u8>> {
    let mp4 = run_ffmpeg(
        limits,
        "mov->mp4",
        "in.mov",
        // The QuickTime/ISO BMFF demuxer, named for every brand it handles.
        "mov,mp4,m4a,3gp,3g2,mj2",
        bytes,
        &["-c", "copy", "-movflags", "faststart"],
    )
    .await?;
    info!(
        "remuxed mov ({} bytes) to mp4 ({} bytes)",
        bytes.len(),
        mp4.len()
    );
    Ok(mp4)
}

/// Write `bytes` to a temp file, run ffmpeg over it under `limits`, and return
/// the result. `label` names the conversion in error messages; `input_format`
/// pins the demuxer.
async fn run_ffmpeg(
    limits: &Limits,
    label: &str,
    in_name: &str,
    input_format: &str,
    bytes: &[u8],
    args: &[&str],
) -> Result<Vec<u8>> {
    // Hold a slot for the whole conversion. Without this, N concurrent uploads
    // mean N concurrent ffmpeg processes, and the box runs out of CPU, memory
    // and temp disk at once.
    let _permit = match tokio::time::timeout(
        limits.queue_timeout,
        Arc::clone(&limits.slots).acquire_owned(),
    )
    .await
    {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => return Err(Error::Internal("transcode semaphore closed".to_string())),
        Err(_) => {
            warn!(
                "transcode queue saturated: {label} waited {}s for a slot",
                limits.queue_timeout.as_secs()
            );
            return Err(Error::RateLimited(
                "video transcoding is at capacity, please retry".to_string(),
            ));
        }
    };

    let dir = tempfile::tempdir()
        .map_err(|e| Error::Internal(format!("transcode tempdir failed: {e}")))?;
    let in_path = dir.path().join(in_name);
    let out_path = dir.path().join("out.mp4");

    let mut infile = tokio::fs::File::create(&in_path)
        .await
        .map_err(|e| Error::Internal(format!("transcode write failed: {e}")))?;
    infile
        .write_all(bytes)
        .await
        .map_err(|e| Error::Internal(format!("transcode write failed: {e}")))?;
    infile
        .flush()
        .await
        .map_err(|e| Error::Internal(format!("transcode flush failed: {e}")))?;
    drop(infile);

    // `-f <input_format>` pins the demuxer to the one the magic bytes already
    // implied, and `-protocol_whitelist file` keeps a crafted input from
    // steering ffmpeg into a playlist-style demuxer that opens other paths or
    // URLs. `-loglevel error` keeps stderr small enough that the process can
    // never block on a full pipe while we supervise it.
    let mut child = Command::new("ffmpeg")
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-nostats")
        .arg("-loglevel")
        .arg("error")
        .arg("-protocol_whitelist")
        .arg("file")
        .arg("-f")
        .arg(input_format)
        .arg("-y")
        .arg("-i")
        .arg(&in_path)
        .arg("-threads")
        .arg(limits.threads.to_string())
        .args(args)
        .arg(&out_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        // If the handler future is dropped -- a client that disconnects
        // mid-upload, or shutdown -- the child dies with it instead of
        // lingering as an orphan burning CPU.
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::Internal(format!("ffmpeg spawn failed: {e}")))?;

    let status = supervise(&mut child, limits, label, &out_path).await?;

    if !status.success() {
        let stderr = capture_stderr(&mut child).await;
        let tail: String = stderr.chars().rev().take(300).collect::<String>();
        let tail: String = tail.chars().rev().collect();
        return Err(Error::Internal(format!(
            "ffmpeg {label} failed ({status}): {tail}"
        )));
    }

    // The poll below can miss a burst of writes between ticks, so re-check the
    // finished file before reading it into memory.
    let written = tokio::fs::metadata(&out_path)
        .await
        .map_err(|e| Error::Internal(format!("transcode stat failed: {e}")))?
        .len();
    if written > limits.max_output_bytes {
        return Err(Error::VideoTooLarge(format!(
            "{label} produced {written} bytes, over the {} byte transcode limit",
            limits.max_output_bytes
        )));
    }

    tokio::fs::read(&out_path)
        .await
        .map_err(|e| Error::Internal(format!("transcode read failed: {e}")))
}

/// Wait for ffmpeg, killing it if it outlives the deadline or its output grows
/// past the ceiling.
///
/// ffmpeg's own `-fs` flag is not used: it is silently ignored for stream-copy
/// remuxes *and* re-encodes (verified against ffmpeg 8.1), so the ceiling is
/// enforced here by watching the file and killing the process.
async fn supervise(
    child: &mut Child,
    limits: &Limits,
    label: &str,
    out_path: &Path,
) -> Result<std::process::ExitStatus> {
    enum Outcome {
        Exited(std::io::Result<std::process::ExitStatus>),
        Deadline,
        TooBig(u64),
    }

    let deadline = tokio::time::sleep(limits.timeout);
    tokio::pin!(deadline);
    let mut poll = tokio::time::interval(OUTPUT_POLL_INTERVAL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Scoped so the `wait()` borrow of `child` ends before we kill it.
    let outcome = {
        let waiter = child.wait();
        tokio::pin!(waiter);
        loop {
            tokio::select! {
                status = &mut waiter => break Outcome::Exited(status),
                _ = &mut deadline => break Outcome::Deadline,
                _ = poll.tick() => {
                    if let Ok(meta) = tokio::fs::metadata(out_path).await {
                        if meta.len() > limits.max_output_bytes {
                            break Outcome::TooBig(meta.len());
                        }
                    }
                }
            }
        }
    };

    match outcome {
        Outcome::Exited(status) => {
            status.map_err(|e| Error::Internal(format!("ffmpeg {label} wait failed: {e}")))
        }
        Outcome::Deadline => {
            kill(child, label, "deadline exceeded").await;
            Err(Error::Internal(format!(
                "ffmpeg {label} exceeded the {}s transcode deadline",
                limits.timeout.as_secs()
            )))
        }
        Outcome::TooBig(written) => {
            kill(child, label, "output ceiling exceeded").await;
            Err(Error::VideoTooLarge(format!(
                "{label} output grew past {} bytes ({written} written)",
                limits.max_output_bytes
            )))
        }
    }
}

/// SIGKILL the child and reap it, so no ffmpeg survives a rejected conversion.
async fn kill(child: &mut Child, label: &str, reason: &str) {
    warn!("killing ffmpeg {label}: {reason}");
    if let Err(e) = child.kill().await {
        warn!("failed to kill ffmpeg {label}: {e}");
    }
}

/// Read at most [`STDERR_CAPTURE_LIMIT`] of ffmpeg's stderr for diagnostics.
async fn capture_stderr(child: &mut Child) -> String {
    let Some(mut pipe) = child.stderr.take() else {
        return String::new();
    };
    let mut buf = Vec::new();
    if let Err(e) = (&mut pipe)
        .take(STDERR_CAPTURE_LIMIT)
        .read_to_end(&mut buf)
        .await
    {
        warn!("failed to read ffmpeg stderr: {e}");
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generous ceilings, so limit tests can tighten only the one they exercise.
    fn test_limits() -> Limits {
        Limits {
            timeout: Duration::from_secs(60),
            queue_timeout: Duration::from_secs(5),
            max_output_bytes: 64 * 1024 * 1024,
            threads: 2,
            slots: Arc::new(Semaphore::new(2)),
        }
    }

    /// Build a fixture with ffmpeg and return its bytes.
    async fn ffmpeg_fixture(name: &str, args: &[&str]) -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        let status = Command::new("ffmpeg")
            .args(args)
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "generating {name} failed");
        tokio::fs::read(&path).await.unwrap()
    }

    async fn mov_fixture() -> Vec<u8> {
        ffmpeg_fixture(
            "fixture.mov",
            &[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                "-t",
                "1",
                "-f",
                "mov",
            ],
        )
        .await
    }

    async fn gif_fixture() -> Vec<u8> {
        ffmpeg_fixture(
            "fixture.gif",
            &[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=25",
                "-t",
                "2",
                "-f",
                "gif",
            ],
        )
        .await
    }

    #[test]
    fn detects_gif_magic() {
        assert!(is_gif(b"GIF89a\x01\x02"));
        assert!(is_gif(b"GIF87a\x01\x02"));
        assert!(!is_gif(b"\x89PNG\r\n"));
        assert!(!is_gif(b"\x00\x00\x00\x1cftypmp42"));
        assert!(!is_gif(b""));
    }

    #[test]
    fn detects_quicktime_brand() {
        // What an iPhone capture / screen recording actually starts with.
        assert!(is_quicktime_container(
            b"\x00\x00\x00\x14ftypqt  \x00\x00\x00\x00"
        ));
    }

    #[test]
    fn iso_bmff_brands_are_not_quicktime() {
        // Already sniff as video/mp4 on the PDS -- must not be remuxed.
        assert!(!is_quicktime_container(
            b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00"
        ));
        assert!(!is_quicktime_container(
            b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00"
        ));
    }

    #[test]
    fn detects_ftyp_less_quicktime() {
        // Older MOVs lead with a bare box; both PDS sniffers call these
        // video/quicktime, so they need the remux too.
        for tag in [b"moov", b"mdat", b"free", b"wide"] {
            let mut buf = vec![0x00, 0x00, 0x00, 0x14];
            buf.extend_from_slice(tag);
            buf.extend_from_slice(&[0u8; 8]);
            assert!(is_quicktime_container(&buf), "expected {tag:?} to match");
        }
    }

    /// End-to-end proof that the remux produces bytes the PDS will tag
    /// `video/mp4`: builds an H.264/AAC QuickTime file the way iOS ships one,
    /// runs it through `mov_to_mp4`, and checks the container brand flipped.
    ///
    /// Run from the repository root with:
    /// `cargo test -p rsky-video mov_remux -- --ignored`
    #[tokio::test]
    #[ignore = "requires ffmpeg on PATH"]
    async fn mov_remux_yields_an_mp4_brand() {
        let mov = mov_fixture().await;
        assert!(
            is_quicktime_container(&mov),
            "ffmpeg -f mov should emit the `qt  ` brand"
        );

        let mp4 = mov_to_mp4(&test_limits(), &mov)
            .await
            .expect("remux should succeed");
        assert_eq!(&mp4[4..8], b"ftyp", "output should be ISO BMFF");
        assert!(
            !is_quicktime_container(&mp4),
            "output still sniffs as QuickTime: brand {:?}",
            String::from_utf8_lossy(&mp4[8..12])
        );
    }

    /// A saturated queue must turn uploads away rather than pile up unbounded
    /// waiters. No ffmpeg runs here -- the slot wait fails first.
    #[tokio::test]
    async fn saturated_queue_is_rejected_with_429() {
        let mut limits = test_limits();
        limits.slots = Arc::new(Semaphore::new(1));
        limits.queue_timeout = Duration::from_millis(50);

        let held = Arc::clone(&limits.slots).acquire_owned().await.unwrap();
        assert_eq!(limits.available_slots(), 0);

        let err = mov_to_mp4(&limits, b"\x00\x00\x00\x14ftypqt  \x00\x00\x00\x00")
            .await
            .expect_err("should be turned away while the only slot is held");
        assert!(
            matches!(err, Error::RateLimited(_)),
            "expected RateLimited, got: {err}"
        );

        drop(held);
        assert_eq!(limits.available_slots(), 1);
    }

    /// The GIF path is already in production, so cover it end to end with the
    /// pinned demuxer and protocol whitelist in place.
    #[tokio::test]
    #[ignore = "requires ffmpeg on PATH"]
    async fn gif_converts_to_an_mp4() {
        let gif = gif_fixture().await;
        assert!(is_gif(&gif));

        let mp4 = gif_to_mp4(&test_limits(), &gif)
            .await
            .expect("gif conversion should succeed");
        assert_eq!(&mp4[4..8], b"ftyp", "output should be ISO BMFF");
        assert!(!is_quicktime_container(&mp4));
    }

    /// The deadline must kill a conversion that outlives it, and the permit
    /// must come back afterward so one slow upload cannot wedge a slot.
    #[tokio::test]
    #[ignore = "requires ffmpeg on PATH"]
    async fn deadline_kills_a_slow_transcode() {
        let gif = gif_fixture().await;
        let mut limits = test_limits();
        limits.timeout = Duration::from_millis(1);

        let err = gif_to_mp4(&limits, &gif)
            .await
            .expect_err("1ms is not enough to re-encode a 2s gif");
        assert!(
            err.to_string().contains("deadline"),
            "expected a deadline error, got: {err}"
        );
        assert_eq!(
            limits.available_slots(),
            2,
            "the permit should be released after a killed transcode"
        );
    }

    /// Output ceiling: guards against a small input expanding into an enormous
    /// output. ffmpeg's own `-fs` does not enforce this (see `supervise`).
    #[tokio::test]
    #[ignore = "requires ffmpeg on PATH"]
    async fn output_ceiling_rejects_oversized_output() {
        let gif = gif_fixture().await;
        let mut limits = test_limits();
        limits.max_output_bytes = 1000;

        let err = gif_to_mp4(&limits, &gif)
            .await
            .expect_err("a 2s 320x240 encode is larger than 1000 bytes");
        assert!(
            matches!(err, Error::VideoTooLarge(_)),
            "expected VideoTooLarge, got: {err}"
        );
    }

    #[test]
    fn non_quicktime_payloads_are_rejected() {
        assert!(!is_quicktime_container(b""));
        assert!(!is_quicktime_container(b"GIF89a\x00\x00\x00\x00\x00\x00"));
        assert!(!is_quicktime_container(
            b"\x00\x00\x00\x14skip\x00\x00\x00\x00"
        ));
        // Shorter than the 12 bytes the brand check needs -- must not panic.
        assert!(!is_quicktime_container(b"\x00\x00\x00\x14ftypqt"));
    }
}
