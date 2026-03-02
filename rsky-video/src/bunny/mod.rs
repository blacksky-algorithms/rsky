//! Bunny Stream API client

mod types;

pub use types::*;

use crate::error::{Error, Result};
use bytes::Bytes;
use tracing::{debug, info};

const BUNNY_API_BASE: &str = "https://video.bunnycdn.com";

/// Client for interacting with Bunny Stream API
#[derive(Debug, Clone)]
pub struct BunnyClient {
    library_id: String,
    api_key: String,
    pull_zone: String,
    client: reqwest::Client,
}

impl BunnyClient {
    pub fn new(library_id: String, api_key: String, pull_zone: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            library_id,
            api_key,
            pull_zone,
            client,
        }
    }

    /// Create a new video object in Bunny Stream
    /// Returns the video GUID that can be used for uploading
    pub async fn create_video(&self, title: &str) -> Result<CreateVideoResponse> {
        let url = format!("{}/library/{}/videos", BUNNY_API_BASE, self.library_id);

        debug!("Creating video in Bunny: {}", title);

        let response = self
            .client
            .post(&url)
            .header("AccessKey", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "title": title
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::BunnyApi(format!(
                "Failed to create video: {} - {}",
                status, body
            )));
        }

        let video: CreateVideoResponse = response.json().await?;
        info!("Created Bunny video: {}", video.guid);
        Ok(video)
    }

    /// Upload video binary data to Bunny Stream
    pub async fn upload_video(&self, video_id: &str, data: Bytes) -> Result<()> {
        let url = format!(
            "{}/library/{}/videos/{}",
            BUNNY_API_BASE, self.library_id, video_id
        );

        debug!(
            "Uploading {} bytes to Bunny video: {}",
            data.len(),
            video_id
        );

        let response = self
            .client
            .put(&url)
            .header("AccessKey", &self.api_key)
            .header("Content-Type", "application/octet-stream")
            .body(data)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::BunnyApi(format!(
                "Failed to upload video: {} - {}",
                status, body
            )));
        }

        info!("Uploaded video to Bunny: {}", video_id);
        Ok(())
    }

    /// Get video status from Bunny Stream
    pub async fn get_video(&self, video_id: &str) -> Result<VideoInfo> {
        let url = format!(
            "{}/library/{}/videos/{}",
            BUNNY_API_BASE, self.library_id, video_id
        );

        let response = self
            .client
            .get(&url)
            .header("AccessKey", &self.api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::BunnyApi(format!(
                "Failed to get video: {} - {}",
                status, body
            )));
        }

        Ok(response.json().await?)
    }

    /// Delete a video from Bunny Stream
    pub async fn delete_video(&self, video_id: &str) -> Result<()> {
        let url = format!(
            "{}/library/{}/videos/{}",
            BUNNY_API_BASE, self.library_id, video_id
        );

        let response = self
            .client
            .delete(&url)
            .header("AccessKey", &self.api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::BunnyApi(format!(
                "Failed to delete video: {} - {}",
                status, body
            )));
        }

        info!("Deleted Bunny video: {}", video_id);
        Ok(())
    }

    /// Get the HLS playlist URL for a video
    pub fn get_playlist_url(&self, video_id: &str) -> String {
        format!(
            "https://{}.b-cdn.net/{}/playlist.m3u8",
            self.pull_zone, video_id
        )
    }

    /// Get the thumbnail URL for a video
    pub fn get_thumbnail_url(&self, video_id: &str) -> String {
        format!(
            "https://{}.b-cdn.net/{}/thumbnail.jpg",
            self.pull_zone, video_id
        )
    }

    /// Get the pull zone hostname
    pub fn pull_zone(&self) -> &str {
        &self.pull_zone
    }

    /// Download the original video file from Bunny CDN
    /// Returns the video bytes
    pub async fn download_video(&self, video_id: &str) -> Result<bytes::Bytes> {
        // The original video is available at the CDN URL with /play.mp4 suffix
        let url = format!("https://{}.b-cdn.net/{}/original", self.pull_zone, video_id);

        debug!("Downloading video from Bunny: {}", url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::BunnyApi(format!(
                "Failed to download video: {} - {}",
                status, body
            )));
        }

        let bytes = response.bytes().await?;
        info!("Downloaded {} bytes from Bunny", bytes.len());
        Ok(bytes)
    }
}
