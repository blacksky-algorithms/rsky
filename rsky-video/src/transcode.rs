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
/// at all. Verified against `file-type` 16.5.4 (TypeScript PDS, `core.js:457`
/// and `core.js:1025`) and `infer` 0.15 (rsky-pds, `matchers/video.rs:49`).
///
/// Note this covers only QuickTime; other `ftyp` brands also fail validation
/// without being QuickTime -- see [`needs_mp4_remux`], which is the predicate
/// the upload path actually gates on.
///
/// Known divergence, deliberately not mirrored: `infer` 0.15's `is_mov` has a
/// fourth clause, `bytes[12..16] == "mdat"` (`matchers/video.rs:55`), which
/// fires for a 12-byte leading box followed by `mdat`. It affects rsky-pds
/// only, and only for brands outside `infer`'s `is_mp4` whitelist (that
/// matcher runs first, `map.rs:217`); the `free`/`moov` clauses above already
/// catch the realistic shapes. Mirroring it faithfully would mean duplicating
/// that whitelist here. The mimeType check in `pds::upload_blob_with_token`
/// is what catches this case if it ever occurs in the wild.
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

/// True when the container needs remuxing to MP4 before the PDS sees it.
///
/// The failing condition is not "is QuickTime" but "the PDS sniffer will not
/// report `video/mp4`", so this covers every *video* brand that trips it:
///
/// - QuickTime, via [`is_quicktime_container`] -- iPhone captures.
/// - `M4V `/`M4VH`/`M4VP` -> `video/x-m4v`. Apple exports, Handbrake's m4v
///   presets, and ffmpeg's own `ipod` muxer. Rejected by *both* sniffers
///   (`file-type` `core.js:480`, `infer` `is_m4v` at `matchers/video.rs:2`).
/// - `3g*` -> `video/3gpp` / `video/3gpp2`. Older Android capture. Rejected by
///   the TypeScript PDS (`core.js:500`); `infer` has no 3GPP matcher at all,
///   so rsky-pds falls back to the content-type we send and lets these pass.
///
/// All of these are H.264/AAC in an ISO BMFF box in practice, so the same
/// stream copy [`mov_to_mp4`] performs handles them -- verified end to end
/// against both sniffer libraries.
///
/// Audio-only brands (`M4A `/`M4B `/`F4A `/`F4B `) and image brands
/// (`avif`/`mif1`/`msf1`/`heic`/`heix`/`hevc`/`hevx`/`crx`) also fail
/// validation but are deliberately **not** remuxed: they carry no video track,
/// so converting them would mint a `video/mp4` blob that embeds as a broken
/// video instead of failing. Those surface as an explicit error from the
/// mimeType check in `pds::upload_blob_with_token`.
pub fn needs_mp4_remux(bytes: &[u8]) -> bool {
    if is_quicktime_container(bytes) {
        return true;
    }
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let brand = &bytes[8..12];
    matches!(brand, b"M4V " | b"M4VH" | b"M4VP") || brand.starts_with(b"3g")
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

/// Remux a QuickTime or other non-MP4 ISO BMFF container to MP4 without
/// re-encoding. Gated by [`needs_mp4_remux`], so it also receives `M4V ` and
/// `3g*` input; ffmpeg demuxes all of them with the same `mov,mp4,m4a,3gp,3g2`
/// demuxer, and the mp4 muxer writes an `isom` brand regardless of the input.
///
/// `-c copy` keeps the existing H.264/AAC streams and only rewrites the
/// container, so this is lossless and fast -- the cost is copying the bytes
/// once (typically under a second for 100MB). Inputs carrying codecs MP4
/// cannot hold (ProRes, or H.263/AMR in a very old 3GP) fail here with
/// ffmpeg's own error instead of producing a blob the PDS would tag as
/// something `app.bsky.embed.video` rejects.
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

    /// Build a minimal `ftyp` header with the given major brand.
    fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
        let mut buf = vec![0x00, 0x00, 0x00, 0x18];
        buf.extend_from_slice(b"ftyp");
        buf.extend_from_slice(brand);
        buf.extend_from_slice(&[0u8; 8]);
        buf
    }

    #[test]
    fn remuxes_every_non_mp4_video_brand() {
        // QuickTime, plus the brands that sniff as video/x-m4v and video/3gpp*.
        // All observed in production; all rejected by app.bsky.embed.video.
        assert!(needs_mp4_remux(b"\x00\x00\x00\x14ftypqt  \x00\x00\x00\x00"));
        for brand in [b"M4V ", b"M4VH", b"M4VP"] {
            assert!(needs_mp4_remux(&ftyp(brand)), "expected {brand:?} to match");
        }
        for brand in [b"3gp4", b"3gp5", b"3gp6", b"3g2a", b"3g2b"] {
            assert!(needs_mp4_remux(&ftyp(brand)), "expected {brand:?} to match");
        }
        // And the ftyp-less QuickTime shapes.
        for tag in [b"moov", b"mdat", b"free", b"wide"] {
            let mut buf = vec![0x00, 0x00, 0x00, 0x14];
            buf.extend_from_slice(tag);
            buf.extend_from_slice(&[0u8; 8]);
            assert!(needs_mp4_remux(&buf), "expected {tag:?} to match");
        }
    }

    #[test]
    fn leaves_mp4_sniffing_brands_alone() {
        // These already sniff as video/mp4; remuxing them would be pure waste.
        for brand in [
            b"isom", b"mp42", b"mp41", b"iso2", b"avc1", b"dash", b"M4P ",
        ] {
            assert!(
                !needs_mp4_remux(&ftyp(brand)),
                "expected {brand:?} to pass through"
            );
        }
    }

    #[test]
    fn does_not_remux_audio_or_image_brands() {
        // These fail lexicon validation too, but they carry no video track --
        // remuxing would mint a video/mp4 blob that embeds as a broken video.
        // They are meant to surface via the mimeType check on the PDS response.
        for brand in [b"M4A ", b"M4B ", b"F4A ", b"F4B "] {
            assert!(
                !needs_mp4_remux(&ftyp(brand)),
                "audio brand {brand:?} must not be remuxed"
            );
        }
        for brand in [
            b"avif", b"mif1", b"msf1", b"heic", b"heix", b"hevc", b"hevx",
        ] {
            assert!(
                !needs_mp4_remux(&ftyp(brand)),
                "image brand {brand:?} must not be remuxed"
            );
        }
    }

    #[test]
    fn needs_mp4_remux_rejects_junk() {
        assert!(!needs_mp4_remux(b""));
        assert!(!needs_mp4_remux(b"GIF89a\x00\x00\x00\x00\x00\x00"));
        assert!(!needs_mp4_remux(b"\x00\x00\x00\x14skip\x00\x00\x00\x00"));
        assert!(!needs_mp4_remux(b"\x1a\x45\xdf\xa3webm-ish\x00\x00"));
        // Shorter than the 12 bytes the brand check needs -- must not panic.
        assert!(!needs_mp4_remux(b"\x00\x00\x00\x14ftypqt"));
        assert!(!needs_mp4_remux(b"\x00\x00\x00\x14ftyp3g"));
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

    /// Same proof for the other two containers the gate now catches: ffmpeg's
    /// `ipod` muxer emits the `M4V ` brand and `3gp` emits `3gp6`, both of
    /// which a PDS reports as something `app.bsky.embed.video` rejects. Each
    /// must stream-copy into a brand that sniffs as `video/mp4`.
    ///
    /// `cargo test -p rsky-video remux_normalizes -- --ignored`
    #[tokio::test]
    #[ignore = "requires ffmpeg on PATH"]
    async fn remux_normalizes_m4v_and_3gp_brands() {
        let dir = tempfile::tempdir().unwrap();

        for (muxer, name, expected_brand) in [
            ("ipod", "fixture.m4v", b"M4V "),
            ("3gp", "fixture.3gp", b"3gp6"),
        ] {
            let path = dir.path().join(name);
            let status = Command::new("ffmpeg")
                .args([
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=size=320x240:rate=15",
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

            let input = tokio::fs::read(&path).await.unwrap();
            assert_eq!(
                &input[8..12],
                expected_brand,
                "expected ffmpeg -f {muxer} to emit brand {:?}",
                String::from_utf8_lossy(expected_brand)
            );
            assert!(
                needs_mp4_remux(&input),
                "{muxer} output should be gated for remux"
            );
            // Not QuickTime -- these are a distinct failure mode.
            assert!(!is_quicktime_container(&input));

            let mp4 = mov_to_mp4(&input)
                .await
                .unwrap_or_else(|e| panic!("{muxer} remux should succeed: {e}"));
            assert_eq!(&mp4[4..8], b"ftyp");
            assert!(
                !needs_mp4_remux(&mp4),
                "{muxer} output still needs remux: brand {:?}",
                String::from_utf8_lossy(&mp4[8..12])
            );
        }
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
