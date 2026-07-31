use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tracing::info;

use crate::error::{Error, Result};

/// How many ffmpeg encodes may run at once. x264 spawns a thread per core, so
/// without this a burst of concurrent uploads oversubscribes the box
/// superlinearly and stalls every request handler; two encodes keep the CPU
/// busy while bounding the damage. Queued uploads wait here rather than
/// failing.
static ENCODE_SLOTS: Semaphore = Semaphore::const_new(2);

/// Hard ceiling on a single ffmpeg run. Uploads are capped at 100MB by size
/// (duration is not enforced anywhere, so a long low-bitrate clip is
/// possible), but veryfast x264 sustains well over realtime even
/// single-threaded; anything still running at this point is a hung or
/// adversarial input and the process is killed rather than pinning a slot
/// forever.
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Ceiling for ffprobe runs. Probing headers takes milliseconds; this only
/// exists so a wedged probe can't pin an upload.
const FFPROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Verify the external binaries this module shells out to actually exist.
/// Both are resolved via PATH at spawn time, so without this check a missing
/// ffprobe builds and boots clean and then fails every single upload; run it
/// at startup so a bad image/host fails loudly at boot instead.
pub async fn preflight() -> Result<()> {
    for bin in ["ffmpeg", "ffprobe"] {
        let status = Command::new(bin)
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|e| Error::Internal(format!("{bin} is not available: {e}")))?;
        if !status.success() {
            return Err(Error::Internal(format!("{bin} -version failed ({status})")));
        }
    }
    Ok(())
}

/// True when the payload is an animated/static GIF (magic bytes GIF87a/GIF89a).
pub fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
}

/// Convert a GIF to a silent MP4 that Bunny Stream can transcode.
///
/// Bunny's encoder rejects GIF input outright, so GIF uploads are converted
/// here first. yuv420p and even dimensions are required for broad H.264
/// playback; faststart keeps the moov atom at the front for streaming.
pub async fn gif_to_mp4(bytes: &[u8]) -> Result<Vec<u8>> {
    let mp4 = run_ffmpeg(
        "gif->mp4",
        "in.gif",
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

/// Re-encode any video upload into a normalized H.264/AAC MP4.
///
/// Every non-GIF upload goes through this, including input that is already
/// MP4. Passing MP4 bytes through untouched -- and stream-copying QuickTime --
/// shipped blobs whose *frame timestamps* were irregular: copy-mode trims and
/// frame deletions (Apple's editors, Twitter rips) rewrite edit lists rather
/// than the stream, leaving a runt first frame interval (~5ms at 60fps instead
/// of 16.7ms). Bluesky's federated video ingest deterministically rejects such
/// streams -- permanent 404 on video.bsky.app with no retry -- while accepting
/// the same content after one re-encode. Bluesky's own video service
/// re-encodes every upload for exactly this reason.
///
/// Precisely what the re-encode fixes: decoding discards edit lists and
/// sub-frame timestamp offsets, so the output timeline starts clean. It is
/// NOT forced CFR -- genuine variable frame rate (dropped frames at whole-
/// frame gaps, e.g. screen recordings) passes through, which is fine:
/// Bluesky's ingest accepts whole-frame-gap VFR (live-verified 2026-07-31,
/// along with the edit-list findings; controlled experiment on identical
/// content, same account, minutes apart -- lossless remux 404s, this encode
/// ingests in under a minute; see
/// obsidian/Design/video-federation-blacksky-to-bluesky-ingest.md). Forcing
/// CFR was considered and rejected: with a pathological r_frame_rate tag it
/// duplicates frames unboundedly.
///
/// The encode settings mirror that experiment's passing variant: H.264 Main
/// profile / yuv420p for broad decoder support (and to match what Bluesky's
/// own transcoder emits), source frame rate preserved. On top of it:
/// even-dimension scaling (x264 rejects odd sizes in yuv420p), AAC audio (MP4
/// cannot carry the PCM/AMR audio some containers arrive with, so copy would
/// fail exactly where a re-encode succeeds), and explicit stream mapping so a
/// video track is *required* -- audio-only input (`.m4a` and friends) fails
/// here loudly instead of minting a `video/mp4` blob that embeds as a broken
/// video. Subtitle/data tracks are dropped.
///
/// This also replaces the old container gate: QuickTime, `M4V `, `3g*`, and
/// even `.webm`/`.mkv` all decode through the same path and come out as an
/// `isom` MP4 the PDS sniffers accept, so the brand no longer matters.
pub async fn reencode_to_mp4(bytes: &[u8]) -> Result<Vec<u8>> {
    let mp4 = run_ffmpeg(
        "video->mp4",
        "in.video",
        bytes,
        &[
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-sn",
            "-dn",
            "-c:v",
            "libx264",
            "-profile:v",
            "main",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-vf",
            &video_filter(bytes).await,
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-movflags",
            "faststart",
        ],
    )
    .await?;

    // `-map 0:v:0` only guarantees *a* video stream exists; ffmpeg happily
    // exposes a still image (PNG/JPEG/HEIC) as a one-frame video stream, so
    // without this check an image posted to uploadVideo would mint a valid
    // `video/mp4` blob that embeds as a broken single-frame video. The old
    // passthrough rejected those loudly at the PDS mimeType check; keep that
    // guarantee by refusing any output that isn't a real moving picture.
    let frames = video_frame_count(&mp4).await?;
    if frames < 2 {
        return Err(Error::BadRequest(format!(
            "input is not a video ({frames} frame(s) after normalization)"
        )));
    }

    info!(
        "re-encoded video ({} bytes) to normalized mp4 ({} bytes, {} frames)",
        bytes.len(),
        mp4.len(),
        frames
    );
    Ok(mp4)
}

/// Build the normalization filter chain, conditioned on the input's tagged
/// color matrix.
///
/// The goal: every output is honestly-tagged bt709 SDR. Without any color
/// handling, a 10-bit BT.2020/HLG source (iPhone default) came out
/// bit-crushed to 8-bit but still *tagged* HDR, so players applied HLG
/// display mapping to SDR data. The conversion must be conditional, though:
/// swscale's fallback for an *untagged* input matrix is bt601 with no SD/HD
/// heuristic, so an unconditional `out_color_matrix=bt709` applies a
/// spurious bt601->bt709 rotation to untagged HD files -- and untagged mp4s
/// (Twitter rips, web re-encodes) are common. So: convert only when the
/// input is tagged with a non-bt709 matrix; otherwise leave pixels alone and
/// stamp tags only.
///
/// The tags-only half is still a known half-fix for HDR: after conversion
/// the matrix is right, but HLG/PQ pixels keep their transfer curve and
/// BT.2020 primaries while tagged bt709, so they render flat and
/// undersaturated (un-mapped, where before they were double-mapped) until a
/// real tonemap lands. Full gamut/transfer tonemapping needs zscale/libzimg,
/// which not every ffmpeg build carries.
async fn video_filter(bytes: &[u8]) -> String {
    const SCALE: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";
    const TAGS: &str = "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709";
    match input_color_space(bytes).await.as_deref() {
        Some("unknown") | Some("") | Some("bt709") | None => format!("{SCALE},{TAGS}"),
        Some(_) => format!("{SCALE}:out_color_matrix=bt709,{TAGS}"),
    }
}

/// The input's tagged color matrix (`color_space`) per ffprobe, or None if
/// probing fails. Probes from a temp file rather than a pipe because
/// arbitrary uploads put the moov atom at the end, which a pipe can't seek
/// to. Failure is not fatal -- the caller just skips matrix conversion.
async fn input_color_space(bytes: &[u8]) -> Option<String> {
    let dir = tempfile::tempdir().ok()?;
    let path = dir.path().join("probe.video");
    tokio::fs::write(&path, bytes).await.ok()?;
    let output = tokio::time::timeout(
        FFPROBE_TIMEOUT,
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=color_space",
                "-of",
                "csv=p=0",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Count the frames in the first video stream of an in-memory MP4 by piping
/// it to ffprobe. The input is always our own faststart output (moov before
/// mdat), so ffprobe can read the count from the headers without seeking.
async fn video_frame_count(mp4: &[u8]) -> Result<u64> {
    let mut child = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_frames",
            "-of",
            "csv=p=0",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::Internal(format!("ffprobe spawn failed: {e}")))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Internal("ffprobe stdin unavailable".to_string()))?;
    // ffprobe stops reading once it has the moov headers; a write error past
    // that point is expected, not a failure.
    let _ = stdin.write_all(mp4).await;
    drop(stdin);

    let output = tokio::time::timeout(FFPROBE_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| Error::Internal("ffprobe timed out".to_string()))?
        .map_err(|e| Error::Internal(format!("ffprobe failed: {e}")))?;

    if !output.status.success() {
        return Err(Error::Internal(format!(
            "ffprobe failed ({})",
            output.status
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| Error::Internal("ffprobe returned no frame count".to_string()))
}

/// Write `bytes` to a temp file, run `ffmpeg -y -i <in> <args> out.mp4`, and
/// return the result. `label` names the conversion in error messages.
async fn run_ffmpeg(label: &str, in_name: &str, bytes: &[u8], args: &[&str]) -> Result<Vec<u8>> {
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

    // Bound concurrent encodes; a closed semaphore is impossible here, so the
    // only await is queueing behind other uploads.
    let _slot = ENCODE_SLOTS
        .acquire()
        .await
        .map_err(|e| Error::Internal(format!("encode slot unavailable: {e}")))?;

    let child = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(&in_path)
        .args(args)
        // Cap the encoder's thread pool: x264 defaults to ~1.5x cores, so on
        // a large box even the two-slot semaphore would admit nearly
        // full-machine CPU bursts (and that machine also runs other
        // services). Applied here so every encode -- GIF included -- gets it.
        .args(["-threads", "4"])
        .arg(&out_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::Internal(format!("ffmpeg spawn failed: {e}")))?;

    let output = tokio::time::timeout(FFMPEG_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| {
            // kill_on_drop reaps the hung process when the future is dropped.
            Error::Internal(format!(
                "ffmpeg {label} timed out after {}s",
                FFMPEG_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| Error::Internal(format!("ffmpeg {label} failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.chars().rev().take(300).collect::<String>();
        let tail: String = tail.chars().rev().collect();
        // A non-zero ffmpeg exit almost always means the *upload* is bad --
        // corrupt, truncated, or a codec MP4 can't hold -- so surface it as
        // a 400, not a 500 that pages on user-supplied garbage. A None exit
        // code means the process died to a signal (OOM kill, host trouble):
        // that's our infrastructure, and it stays Internal so it *does* page.
        return Err(match output.status.code() {
            Some(_) => {
                Error::BadRequest(format!("ffmpeg {label} failed ({}): {tail}", output.status))
            }
            None => Error::Internal(format!(
                "ffmpeg {label} killed by signal ({}): {tail}",
                output.status
            )),
        });
    }

    tokio::fs::read(&out_path)
        .await
        .map_err(|e| Error::Internal(format!("transcode read failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_gif_magic() {
        assert!(is_gif(b"GIF89a\x01\x02"));
        assert!(is_gif(b"GIF87a\x01\x02"));
        assert!(!is_gif(b"\x89PNG\r\n"));
        assert!(!is_gif(b"\x00\x00\x00\x1cftypmp42"));
        assert!(!is_gif(b""));
    }

    /// Generate a 1-second H.264/AAC fixture with the given muxer, returning
    /// its bytes. Used by the ignored end-to-end tests below.
    async fn fixture(dir: &std::path::Path, muxer: &str, name: &str) -> Vec<u8> {
        let path = dir.join(name);
        let status = Command::new("ffmpeg")
            .args([
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
                muxer,
            ])
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "{muxer} fixture generation failed");
        tokio::fs::read(&path).await.unwrap()
    }

    /// Presentation-order intervals between the first video packets, in
    /// stream timebase units. The poison pattern this module exists to fix
    /// shows up as a runt first interval (a fraction of the frame duration)
    /// followed by regular steps. The final interval is dropped: reading a
    /// fixed count of packets in decode order truncates the presentation
    /// tail, which fabricates a gap there.
    async fn video_pts_deltas(dir: &std::path::Path, bytes: &[u8]) -> Vec<i64> {
        let path = dir.join("probe.mp4");
        tokio::fs::write(&path, bytes).await.unwrap();
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "packet=pts",
                "-read_intervals",
                "%+#12",
                "-of",
                "csv=p=0",
            ])
            .arg(&path)
            .output()
            .await
            .expect("ffprobe should be on PATH");
        let mut pts: Vec<i64> = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .map(|l| l.parse().expect("ffprobe should report numeric pts"))
            .collect();
        pts.sort_unstable();
        let mut deltas: Vec<i64> = pts.windows(2).map(|w| w[1] - w[0]).collect();
        deltas.pop();
        deltas
    }

    /// End-to-end proof for the timestamp fix: recreate the wild poison
    /// pattern (all frames except the first shifted 3/4 of a frame earlier,
    /// leaving a runt first interval -- what copy-mode trims and frame
    /// deletions produce), then require the re-encode to emit uniform frame
    /// intervals. Bluesky's federated ingest permanently rejects the former
    /// and accepts the latter.
    ///
    /// `cargo test -p rsky-video reencode_normalizes_timestamps -- --ignored`
    #[tokio::test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    async fn reencode_normalizes_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path(), "mp4", "clean.mp4").await;

        // 30 fps in x264's default 15360 timebase = 512 ticks per frame;
        // shifting all but the first packet back 384 ticks leaves a 128-tick
        // (quarter-frame) first interval, matching the failing wild blobs.
        let src = dir.path().join("clean.mp4");
        let poison_path = dir.path().join("poison.mp4");
        let status = Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(&src)
            .args([
                "-c",
                "copy",
                "-bsf:v",
                "setts=pts=if(eq(N\\,0)\\,PTS\\,PTS-384):dts=DTS-384",
            ])
            .arg(&poison_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "poison fixture generation failed");
        let poison = tokio::fs::read(&poison_path).await.unwrap();

        let deltas = video_pts_deltas(dir.path(), &poison).await;
        assert!(
            deltas.iter().min() < deltas.iter().max(),
            "fixture should reproduce the runt-interval poison: {deltas:?}"
        );

        let normalized = reencode_to_mp4(&poison)
            .await
            .expect("re-encode should succeed");
        assert_eq!(&normalized[4..8], b"ftyp", "output should be ISO BMFF");
        let deltas = video_pts_deltas(dir.path(), &normalized).await;
        assert!(
            !deltas.is_empty() && deltas.iter().min() == deltas.iter().max(),
            "re-encode must emit uniform frame intervals, got {deltas:?}"
        );
    }

    /// The re-encode also subsumes the old container remux: QuickTime, M4V,
    /// and 3GP input (all H.264/AAC in practice) must come out as an MP4 brand
    /// the PDS sniffers tag `video/mp4`.
    ///
    /// `cargo test -p rsky-video reencode_normalizes_containers -- --ignored`
    #[tokio::test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    async fn reencode_normalizes_containers() {
        let dir = tempfile::tempdir().unwrap();
        for (muxer, name) in [
            ("mov", "fixture.mov"),
            ("ipod", "fixture.m4v"),
            ("3gp", "fixture.3gp"),
        ] {
            let input = fixture(dir.path(), muxer, name).await;
            let mp4 = reencode_to_mp4(&input)
                .await
                .unwrap_or_else(|e| panic!("{muxer} re-encode should succeed: {e}"));
            assert_eq!(&mp4[4..8], b"ftyp", "{muxer} output should be ISO BMFF");
            assert_eq!(
                &mp4[8..12],
                b"isom",
                "{muxer} output brand should sniff as video/mp4"
            );
        }
    }

    /// HDR input must come out as honestly-tagged SDR. Without the color
    /// conversion in the filter chain, a 10-bit BT.2020/HLG source (iPhone
    /// default since the 12) was bit-crushed to 8-bit but kept its HDR tags,
    /// so players applied HLG display mapping to SDR data.
    ///
    /// `cargo test -p rsky-video reencode_tags_bt709 -- --ignored`
    #[tokio::test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    async fn reencode_tags_bt709() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hlg.mp4");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30",
                "-t",
                "1",
                "-vf",
                "setparams=colorspace=bt2020nc:color_primaries=bt2020:color_trc=arib-std-b67,\
                 format=yuv420p10le",
                "-c:v",
                "libx264",
            ])
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "hlg fixture generation failed");

        let hlg = tokio::fs::read(&path).await.unwrap();
        let out = reencode_to_mp4(&hlg)
            .await
            .expect("re-encode should succeed");
        let probe_path = dir.path().join("out.mp4");
        tokio::fs::write(&probe_path, &out).await.unwrap();
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=pix_fmt,color_space,color_transfer,color_primaries",
                "-of",
                "csv=p=0",
            ])
            .arg(&probe_path)
            .output()
            .await
            .expect("ffprobe should be on PATH");
        let tags = String::from_utf8_lossy(&probe.stdout);
        assert_eq!(
            tags.trim(),
            "yuv420p,bt709,bt709,bt709",
            "HDR input must come out as tagged bt709 SDR, got: {tags}"
        );
    }

    /// Untagged input (no colr atom -- typical Twitter rip / web re-encode)
    /// must NOT get a matrix conversion: swscale's fallback for an untagged
    /// input matrix is bt601, so an unconditional out_color_matrix=bt709
    /// applies a spurious color rotation. The conditional chain must produce
    /// byte-identical output for untagged and explicitly-bt709-tagged
    /// variants of the same pixels (both skip conversion).
    ///
    /// `cargo test -p rsky-video reencode_leaves_untagged_colors_alone -- --ignored`
    #[tokio::test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    async fn reencode_leaves_untagged_colors_alone() {
        let dir = tempfile::tempdir().unwrap();
        let untagged_path = dir.path().join("untagged.mp4");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=30",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&untagged_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "untagged fixture generation failed");

        // Same pixels, explicitly tagged bt709.
        let tagged_path = dir.path().join("tagged.mp4");
        let status = Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(&untagged_path)
            .args([
                "-c:v",
                "copy",
                "-bsf:v",
                "h264_metadata=colour_primaries=1:transfer_characteristics=1:matrix_coefficients=1",
            ])
            .arg(&tagged_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "bt709-tagged fixture generation failed");

        let untagged = tokio::fs::read(&untagged_path).await.unwrap();
        let tagged = tokio::fs::read(&tagged_path).await.unwrap();
        let out_untagged = reencode_to_mp4(&untagged).await.unwrap();
        let out_tagged = reencode_to_mp4(&tagged).await.unwrap();
        assert_eq!(
            out_untagged, out_tagged,
            "untagged input must skip matrix conversion (bt601 fallback would \
             shift colors); outputs of untagged vs bt709-tagged pixels differ"
        );
    }

    /// Still-image input must fail loudly: ffmpeg exposes a PNG/JPEG as a
    /// one-frame video stream, so `-map 0:v:0` alone would mint a degenerate
    /// `video/mp4` blob. The frame-count check is what rejects it.
    ///
    /// `cargo test -p rsky-video reencode_rejects_still_image -- --ignored`
    #[tokio::test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    async fn reencode_rejects_still_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("still.png");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=red:size=100x100",
                "-frames:v",
                "1",
            ])
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "png fixture generation failed");

        let png = tokio::fs::read(&path).await.unwrap();
        assert!(
            reencode_to_mp4(&png).await.is_err(),
            "a still image must not produce a video blob"
        );
    }

    /// Audio-only input must fail loudly rather than mint a `video/mp4` blob
    /// that embeds as a broken video (the `-map 0:v:0` requirement).
    ///
    /// `cargo test -p rsky-video reencode_rejects_audio_only -- --ignored`
    #[tokio::test]
    #[ignore = "requires ffmpeg on PATH"]
    async fn reencode_rejects_audio_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.m4a");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440",
                "-c:a",
                "aac",
                "-t",
                "1",
                "-f",
                "ipod",
            ])
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "audio fixture generation failed");

        let audio = tokio::fs::read(&path).await.unwrap();
        assert!(
            reencode_to_mp4(&audio).await.is_err(),
            "audio-only input must not produce a video blob"
        );
    }
}
