use anyhow::Result;
use image::ImageReader;
use std::io::Cursor;

pub struct ImageInfo {
    pub height: u32,
    pub width: u32,
    pub size: Option<u32>,
    pub mime: String,
}

pub async fn mime_type_from_bytes(bytes: Vec<u8>) -> Result<Option<String>> {
    match infer::get(bytes.as_slice()) {
        Some(kind) => Ok(Some(kind.mime_type().to_string())),
        None => Ok(None),
    }
}

/// Dimensions and type read from an image's header alone.
fn info_from_reader<R: std::io::BufRead + std::io::Seek>(
    reader: ImageReader<R>,
) -> Result<ImageInfo> {
    let reader = reader.with_guessed_format()?;
    let mime = reader
        .format()
        .ok_or_else(|| anyhow::anyhow!("unknown image format"))?
        .to_mime_type()
        .to_string();
    let (width, height) = reader.into_dimensions()?;
    Ok(ImageInfo {
        height,
        width,
        size: None,
        mime,
    })
}

pub async fn maybe_get_info_from_path(path: std::path::PathBuf) -> Result<Option<ImageInfo>> {
    let info = tokio::task::spawn_blocking(move || -> Result<ImageInfo> {
        info_from_reader(ImageReader::open(&path)?)
    })
    .await?;
    Ok(info.ok())
}

pub async fn maybe_get_info(bytes: Vec<u8>) -> Result<Option<ImageInfo>> {
    Ok(info_from_reader(ImageReader::new(Cursor::new(bytes))).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[tokio::test]
    async fn dimensions_come_from_the_header_for_bytes_and_files() {
        let info = maybe_get_info(png()).await.unwrap().unwrap();
        assert_eq!(
            (info.width, info.height, info.mime.as_str()),
            (3, 2, "image/png")
        );
        assert!(maybe_get_info(b"not an image".to_vec())
            .await
            .unwrap()
            .is_none());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pic.png");
        std::fs::write(&path, png()).unwrap();
        let info = maybe_get_info_from_path(path).await.unwrap().unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert!(maybe_get_info_from_path(dir.path().join("missing"))
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            mime_type_from_bytes(png()).await.unwrap().as_deref(),
            Some("image/png")
        );
        assert!(mime_type_from_bytes(vec![0, 1, 2]).await.unwrap().is_none());
    }
}
