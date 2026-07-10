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
    let dir = tempfile::tempdir()
        .map_err(|e| Error::Internal(format!("transcode tempdir failed: {e}")))?;
    let in_path = dir.path().join("in.gif");
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
        .arg("-movflags")
        .arg("faststart")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-vf")
        .arg("scale=trunc(iw/2)*2:trunc(ih/2)*2")
        .arg("-an")
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
            "ffmpeg gif->mp4 failed ({}): {tail}",
            output.status
        )));
    }

    let mp4 = tokio::fs::read(&out_path)
        .await
        .map_err(|e| Error::Internal(format!("transcode read failed: {e}")))?;
    info!(
        "transcoded gif ({} bytes) to mp4 ({} bytes)",
        bytes.len(),
        mp4.len()
    );
    Ok(mp4)
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
}
