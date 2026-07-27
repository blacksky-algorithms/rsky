use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::info;

use crate::error::{Error, Result};

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

/// Remux a QuickTime `.mov` to MP4 without re-encoding.
///
/// `-c copy` keeps the existing H.264/AAC streams and only rewrites the
/// container, so this is lossless and fast -- the cost is copying the bytes
/// once (typically under a second for 100MB). MOVs carrying codecs MP4 cannot
/// hold (ProRes, for instance) fail here with ffmpeg's own error instead of
/// producing a blob the PDS would tag `video/quicktime`.
pub async fn mov_to_mp4(bytes: &[u8]) -> Result<Vec<u8>> {
    let mp4 = run_ffmpeg(
        "mov->mp4",
        "in.mov",
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
        let dir = tempfile::tempdir().unwrap();
        let mov_path = dir.path().join("fixture.mov");
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
                "mov",
            ])
            .arg(&mov_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .expect("ffmpeg should be on PATH");
        assert!(status.success(), "fixture generation failed");

        let mov = tokio::fs::read(&mov_path).await.unwrap();
        assert!(
            is_quicktime_container(&mov),
            "ffmpeg -f mov should emit the `qt  ` brand"
        );

        let mp4 = mov_to_mp4(&mov).await.expect("remux should succeed");
        assert_eq!(&mp4[4..8], b"ftyp", "output should be ISO BMFF");
        assert!(
            !is_quicktime_container(&mp4),
            "output still sniffs as QuickTime: brand {:?}",
            String::from_utf8_lossy(&mp4[8..12])
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
