//! Video transcoding via ffmpeg subprocess (GIF re-encode, MOV remux).

use bytes::Bytes;
use tracing::{debug, info};

use crate::error::{Error, Result};

/// GIF87a and GIF89a magic bytes
const GIF_MAGIC_87A: &[u8] = b"GIF87a";
const GIF_MAGIC_89A: &[u8] = b"GIF89a";

/// Check if a file needs conversion before uploading to Bunny.
/// Returns true for GIF files (detected by both extension and magic bytes).
pub fn needs_conversion(filename: &str, data: &[u8]) -> bool {
    let ext_match = filename
        .rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"));

    let magic_match =
        data.len() >= 6 && (data.starts_with(GIF_MAGIC_87A) || data.starts_with(GIF_MAGIC_89A));

    ext_match && magic_match
}

/// Detect MIME type from filename extension.
pub fn detect_mime_type(filename: &str) -> &'static str {
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "gif" => "image/gif",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        _ => "video/mp4",
    }
}

/// Convert GIF bytes to MP4 via ffmpeg subprocess.
///
/// Uses `-movflags +faststart` for streaming, `-pix_fmt yuv420p` for
/// broad compatibility, and scales to even dimensions (required by yuv420p).
pub async fn convert_gif_to_mp4(ffmpeg_path: &str, data: Bytes) -> Result<Bytes> {
    let temp_dir = tempfile::tempdir()
        .map_err(|e| Error::TranscodeFailed(format!("Failed to create temp dir: {}", e)))?;

    let input_path = temp_dir.path().join("input.gif");
    let output_path = temp_dir.path().join("output.mp4");

    debug!("Converting GIF ({} bytes) to MP4 via ffmpeg", data.len());

    tokio::fs::write(&input_path, &data)
        .await
        .map_err(|e| Error::TranscodeFailed(format!("Failed to write temp input: {}", e)))?;

    let output = tokio::process::Command::new(ffmpeg_path)
        .args([
            "-i",
            input_path.to_str().unwrap(),
            "-movflags",
            "+faststart",
            "-pix_fmt",
            "yuv420p",
            "-vf",
            "scale=trunc(iw/2)*2:trunc(ih/2)*2",
            "-y",
            output_path.to_str().unwrap(),
        ])
        .output()
        .await
        .map_err(|e| {
            Error::TranscodeFailed(format!(
                "Failed to run ffmpeg (is it installed at '{}'?): {}",
                ffmpeg_path, e
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::TranscodeFailed(format!(
            "ffmpeg exited with {}: {}",
            output.status,
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    let mp4_bytes = tokio::fs::read(&output_path)
        .await
        .map_err(|e| Error::TranscodeFailed(format!("Failed to read ffmpeg output: {}", e)))?;

    info!(
        "GIF converted to MP4: {} bytes -> {} bytes",
        data.len(),
        mp4_bytes.len()
    );

    Ok(Bytes::from(mp4_bytes))
}

/// True if the buffer is an ISO BMFF file with a QuickTime brand (`qt  `).
pub fn is_quicktime_container(data: &[u8]) -> bool {
    data.len() >= 12 && &data[4..8] == b"ftyp" && &data[8..12] == b"qt  "
}

/// Remux MOV -> MP4 without re-encoding (stream copy via ffmpeg `-c copy`).
pub async fn convert_mov_to_mp4(ffmpeg_path: &str, data: Bytes) -> Result<Bytes> {
    let temp_dir =
        tempfile::tempdir().map_err(|e| Error::TranscodeFailed(format!("temp dir: {e}")))?;

    let input_path = temp_dir.path().join("input.mov");
    let output_path = temp_dir.path().join("output.mp4");

    debug!("Remuxing MOV ({} bytes) to MP4 via ffmpeg", data.len());
    let start = std::time::Instant::now();

    tokio::fs::write(&input_path, &data)
        .await
        .map_err(|e| Error::TranscodeFailed(format!("write input: {e}")))?;

    let output = tokio::process::Command::new(ffmpeg_path)
        .args([
            "-i",
            input_path.to_str().unwrap(),
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-y",
            output_path.to_str().unwrap(),
        ])
        .output()
        .await
        .map_err(|e| Error::TranscodeFailed(format!("run ffmpeg ({}): {e}", ffmpeg_path)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::TranscodeFailed(format!(
            "ffmpeg {}: {}",
            output.status,
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    let mp4_bytes = tokio::fs::read(&output_path)
        .await
        .map_err(|e| Error::TranscodeFailed(format!("read output: {e}")))?;

    info!(
        "MOV remuxed to MP4 in {} ms: {} -> {} bytes",
        start.elapsed().as_millis(),
        data.len(),
        mp4_bytes.len()
    );

    Ok(Bytes::from(mp4_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_conversion_gif87a() {
        let data = b"GIF87a\x00\x00\x00\x00";
        assert!(needs_conversion("animation.gif", data));
    }

    #[test]
    fn test_needs_conversion_gif89a() {
        let data = b"GIF89a\x01\x00\x01\x00";
        assert!(needs_conversion("test.GIF", data));
    }

    #[test]
    fn test_needs_conversion_not_gif_extension() {
        let data = b"GIF89a\x01\x00\x01\x00";
        assert!(!needs_conversion("video.mp4", data));
    }

    #[test]
    fn test_needs_conversion_not_gif_magic() {
        let data = b"\x00\x00\x00\x1cftyp";
        assert!(!needs_conversion("file.gif", data));
    }

    #[test]
    fn test_needs_conversion_empty() {
        assert!(!needs_conversion("file.gif", &[]));
    }

    #[test]
    fn test_detect_mime_type() {
        assert_eq!(detect_mime_type("video.mp4"), "video/mp4");
        assert_eq!(detect_mime_type("clip.mov"), "video/quicktime");
        assert_eq!(detect_mime_type("anim.gif"), "image/gif");
        assert_eq!(detect_mime_type("video.webm"), "video/webm");
        assert_eq!(detect_mime_type("unknown.xyz"), "video/mp4");
        assert_eq!(detect_mime_type("noext"), "video/mp4");
    }

    #[test]
    fn quicktime_container_iphone_screen_recording() {
        // size(4) + "ftyp" + "qt  " brand
        let data = b"\x00\x00\x00\x14ftypqt  \x00\x00\x02\x00";
        assert!(is_quicktime_container(data));
    }

    #[test]
    fn mp4_brand_is_not_quicktime() {
        // ISO BMFF with mp42 brand -- already valid video/mp4, no remux needed.
        let data = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00";
        assert!(!is_quicktime_container(data));
    }

    #[test]
    fn isom_brand_is_not_quicktime() {
        let data = b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00";
        assert!(!is_quicktime_container(data));
    }

    #[test]
    fn random_bytes_are_not_quicktime() {
        assert!(!is_quicktime_container(b""));
        assert!(!is_quicktime_container(b"GIF89a\x00\x00\x00\x00\x00\x00"));
        assert!(!is_quicktime_container(b"too-short"));
    }
}
