//! Bunny Stream API client

mod types;

pub use types::*;

use crate::error::{Error, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bytes::Bytes;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{debug, info, warn};

const BUNNY_API_BASE: &str = "https://video.bunnycdn.com";

/// Bunny URL Token Authentication token for a directory (path-style).
///
/// Bunny's advanced token-auth message is:
/// signed_path + expires + sorted_query_params + optional_client_ip.
/// We do not bind to an IP, and the only query parameter being signed is the
/// directory-scoping `token_path`.
fn bunny_dir_token(security_key: &str, token_path: &str, expires: i64) -> String {
    let query_params = format!("token_path={token_path}");
    let message = format!("{token_path}{expires}{query_params}");
    let mut mac = Hmac::<Sha256>::new_from_slice(security_key.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(message.as_bytes());
    format!(
        "HS256-{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

/// Client for interacting with Bunny Stream API
// Deliberately not `Debug`: this client contains Bunny API and CDN signing keys.
#[derive(Clone)]
pub struct BunnyClient {
    library_id: String,
    api_key: String,
    pull_zone: String,
    /// `None` disables URL signing (CDN_TOKEN_AUTH=false, cutover only).
    token_key: Option<String>,
    /// Longest Cache-Control max-age among the 307 redirects that carry the
    /// signed URLs; the token TTL is derived from it.
    longest_redirect_max_age_secs: i64,
    client: reqwest::Client,
}

impl BunnyClient {
    pub fn new(
        library_id: String,
        api_key: String,
        pull_zone: String,
        token_key: Option<String>,
        longest_redirect_max_age_secs: i64,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        if token_key.is_none() {
            warn!("CDN token auth is disabled — emitting unsigned playback URLs");
        }

        Self {
            library_id,
            api_key,
            pull_zone,
            token_key,
            longest_redirect_max_age_secs,
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

    /// HLS playlist URL (when signing, a path-style token covers the whole
    /// /{guid}/ dir).
    pub fn get_playlist_url(&self, video_id: &str) -> String {
        self.dir_url_at(video_id, "playlist.m3u8", self.token_expiry())
    }

    /// Thumbnail URL (same /{guid}/ directory token).
    pub fn get_thumbnail_url(&self, video_id: &str) -> String {
        self.dir_url_at(video_id, "thumbnail.jpg", self.token_expiry())
    }

    /// Token TTL: the longest redirect cache lifetime (a cached 307 must never
    /// carry an expired token) plus a full day of margin for playback time.
    fn token_expiry(&self) -> i64 {
        const PLAYBACK_MARGIN_SECS: i64 = 24 * 3600;
        chrono::Utc::now().timestamp() + self.longest_redirect_max_age_secs + PLAYBACK_MARGIN_SECS
    }

    /// CDN URL for a file under /{guid}/. Signed when token auth is enabled:
    /// the token goes in the path prefix (directory style) so relatively
    /// resolved variant playlists and .ts segments under /{guid}/ inherit it.
    /// Unsigned when disabled (cutover phases where the pull zone does not
    /// enforce token auth yet).
    fn dir_url_at(&self, video_id: &str, file: &str, expires: i64) -> String {
        // `video_id` (the Bunny GUID, looked up from the DB — never the raw
        // `cid` path param) and `pull_zone` (config) are trusted, not user
        // input, so they are interpolated into the URL without escaping.
        let Some(token_key) = &self.token_key else {
            return format!("https://{}.b-cdn.net/{}/{}", self.pull_zone, video_id, file);
        };
        let token_path = format!("/{video_id}/");
        let encoded_token_path = urlencoding::encode(&token_path);
        let token = bunny_dir_token(token_key, &token_path, expires);
        format!(
            "https://{}.b-cdn.net/bcdn_token={}&expires={}&token_path={}/{}/{}",
            self.pull_zone, token, expires, encoded_token_path, video_id, file
        )
    }

    /// Get the pull zone hostname
    pub fn pull_zone(&self) -> &str {
        &self.pull_zone
    }

    /// Download the original video file from Bunny CDN
    /// Returns the video bytes
    pub async fn download_video(&self, video_id: &str) -> Result<bytes::Bytes> {
        let url = self.dir_url_at(video_id, "original", self.token_expiry());

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

#[cfg(test)]
mod tests {
    use super::*;

    fn live_client() -> BunnyClient {
        BunnyClient::new(
            std::env::var("BUNNY_LIBRARY_ID").expect("BUNNY_LIBRARY_ID must be set"),
            std::env::var("BUNNY_API_KEY").expect("BUNNY_API_KEY must be set"),
            std::env::var("BUNNY_PULL_ZONE").expect("BUNNY_PULL_ZONE must be set"),
            Some(std::env::var("BUNNY_TOKEN_KEY").expect("BUNNY_TOKEN_KEY must be set")),
            86400,
        )
    }

    fn test_client(token_key: Option<&str>) -> BunnyClient {
        BunnyClient::new(
            "lib".into(),
            "apikey".into(),
            "vz-test".into(),
            token_key.map(String::from),
            86400,
        )
    }

    #[test]
    fn dir_token_matches_known_answer() {
        let token = bunny_dir_token("testsecretkey", "/abc123/", 1_700_000_000);
        assert_eq!(token, "HS256-3DqDGmwczwdRYRoVZgRG5re_nSVzO_CzErQQqZ3zCGo");
    }

    #[test]
    fn signed_dir_url_is_path_style_and_deterministic() {
        let c = test_client(Some("testsecretkey"));
        let url = c.dir_url_at("abc123", "playlist.m3u8", 1_700_000_000);
        assert_eq!(
            url,
            "https://vz-test.b-cdn.net/bcdn_token=HS256-3DqDGmwczwdRYRoVZgRG5re_nSVzO_CzErQQqZ3zCGo&expires=1700000000&token_path=%2Fabc123%2F/abc123/playlist.m3u8"
        );
    }

    #[test]
    fn unsigned_url_when_token_auth_disabled() {
        let c = test_client(None);
        assert_eq!(
            c.dir_url_at("abc123", "playlist.m3u8", 1_700_000_000),
            "https://vz-test.b-cdn.net/abc123/playlist.m3u8"
        );
    }

    #[test]
    fn cdn_resources_sign_the_same_directory() {
        let c = test_client(Some("testsecretkey"));

        let playlist = c.dir_url_at("abc123", "playlist.m3u8", 1_700_000_000);
        let thumbnail = c.dir_url_at("abc123", "thumbnail.jpg", 1_700_000_000);
        let original = c.dir_url_at("abc123", "original", 1_700_000_000);

        let playlist_token = playlist
            .split("/abc123/playlist.m3u8")
            .next()
            .expect("playlist URL has signed prefix");
        let thumbnail_token = thumbnail
            .split("/abc123/thumbnail.jpg")
            .next()
            .expect("thumbnail URL has signed prefix");
        let original_token = original
            .split("/abc123/original")
            .next()
            .expect("original URL has signed prefix");
        assert_eq!(playlist_token, thumbnail_token);
        assert_eq!(playlist_token, original_token);
    }

    /// Exercises Bunny's real token-auth implementation against a configured
    /// test library. The library must contain at least one encoded video.
    ///
    /// Run from the repository root with:
    /// `set -a; source .env; set +a; cargo test -p rsky-video live_signed_hls -- --ignored`
    #[tokio::test]
    #[ignore = "requires Bunny test-library credentials and network access"]
    async fn live_signed_hls_reaches_thumbnail_playlist_and_segment() {
        let client = live_client();
        let videos_url = format!(
            "{}/library/{}/videos?page=1&itemsPerPage=20&orderBy=date",
            BUNNY_API_BASE, client.library_id
        );
        let response = client
            .client
            .get(videos_url)
            .header("AccessKey", &client.api_key)
            .send()
            .await
            .expect("list videos request failed")
            .error_for_status()
            .expect("list videos returned an error");
        let payload: serde_json::Value = response.json().await.expect("invalid videos response");
        let video_id = payload["items"]
            .as_array()
            .expect("videos response has no items")
            .iter()
            .find(|video| matches!(video["status"].as_i64(), Some(3 | 4)))
            .and_then(|video| video["guid"].as_str())
            .expect("test library has no encoded video");

        let unsigned_thumbnail = format!(
            "https://{}.b-cdn.net/{video_id}/thumbnail.jpg",
            client.pull_zone
        );
        let unsigned_status = client
            .client
            .get(unsigned_thumbnail)
            .send()
            .await
            .expect("unsigned thumbnail probe failed")
            .status();
        assert!(
            !unsigned_status.is_success(),
            "Bunny CDN token authentication is not enabled for the test library"
        );

        let tampered_thumbnail = client
            .get_thumbnail_url(video_id)
            .replacen("HS256-", "HS256-A", 1);
        let tampered_status = client
            .client
            .get(tampered_thumbnail)
            .send()
            .await
            .expect("tampered thumbnail probe failed")
            .status();
        assert!(
            !tampered_status.is_success(),
            "Bunny accepted a tampered CDN token"
        );

        client
            .client
            .get(client.get_thumbnail_url(video_id))
            .send()
            .await
            .expect("thumbnail request failed")
            .error_for_status()
            .expect("signed thumbnail URL was rejected");

        let playlist_url = client.get_playlist_url(video_id);
        let master = client
            .client
            .get(&playlist_url)
            .send()
            .await
            .expect("master playlist request failed")
            .error_for_status()
            .expect("signed master playlist URL was rejected")
            .text()
            .await
            .expect("master playlist was not text");
        let variant_path = master
            .lines()
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .expect("master playlist has no variant");
        let variant_url = reqwest::Url::parse(&playlist_url)
            .expect("invalid signed playlist URL")
            .join(variant_path)
            .expect("invalid variant path");
        let variant = client
            .client
            .get(variant_url.clone())
            .send()
            .await
            .expect("variant playlist request failed")
            .error_for_status()
            .expect("signed variant playlist URL was rejected")
            .text()
            .await
            .expect("variant playlist was not text");
        let segment_path = variant
            .lines()
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .expect("variant playlist has no segment");
        let segment_url = variant_url
            .join(segment_path)
            .expect("invalid segment path");
        client
            .client
            .get(segment_url)
            .send()
            .await
            .expect("segment request failed")
            .error_for_status()
            .expect("signed media segment URL was rejected");
    }
}
