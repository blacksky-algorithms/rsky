use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::info;

use crate::error::{Error, Result};

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
/// the same content after one re-encode, which regenerates the timeline.
/// Bluesky's own video service re-encodes every upload for exactly this
/// reason. Proven by controlled experiment on 2026-07-31: identical content,
/// same account, minutes apart -- lossless remux 404s, this encode ingests in
/// under a minute (see obsidian/Design/video-federation-blacksky-to-bluesky-ingest.md).
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
            "scale=trunc(iw/2)*2:trunc(ih/2)*2",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-movflags",
            "faststart",
        ],
    )
    .await?;
    info!(
        "re-encoded video ({} bytes) to normalized mp4 ({} bytes)",
        bytes.len(),
        mp4.len()
    );
    Ok(mp4)
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

    let output = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(&in_path)
        .args(args)
        .arg(&out_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("ffmpeg spawn failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.chars().rev().take(300).collect::<String>();
        let tail: String = tail.chars().rev().collect();
        return Err(Error::Internal(format!(
            "ffmpeg {label} failed ({}): {tail}",
            output.status
        )));
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
                "-y", "-f", "lavfi", "-i", "sine=frequency=440", "-c:a", "aac", "-t", "1", "-f",
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
