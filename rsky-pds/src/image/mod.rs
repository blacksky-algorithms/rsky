use anyhow::Result;
use image::io::Reader as ImageReader;
use image::{guess_format, GenericImageView};
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

pub async fn maybe_get_info(bytes: Vec<u8>) -> Result<Option<ImageInfo>> {
    let process_image = || -> Result<Option<ImageInfo>> {
        let img = ImageReader::new(Cursor::new(bytes.clone()))
            .with_guessed_format()?
            .decode()?;
        let (width, height) = img.dimensions();
        let mime = guess_format(bytes.as_slice())?.to_mime_type().to_string();
        let size: Option<u32> = None;
        Ok(Some(ImageInfo {
            height,
            width,
            size,
            mime,
        }))
    };

    return match process_image() {
        Ok(info) => Ok(info),
        Err(_) => Ok(None),
    };
}
