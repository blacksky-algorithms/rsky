//! Bunny Stream API types

use serde::{Deserialize, Serialize};

/// Response from creating a new video
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVideoResponse {
    /// Unique video identifier (GUID)
    pub guid: String,
    /// Video title
    pub title: Option<String>,
    /// Library ID the video belongs to
    pub video_library_id: i64,
}

/// Video information from Bunny Stream
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    /// Unique video identifier
    pub guid: String,
    /// Video title
    pub title: Option<String>,
    /// Video library ID
    pub video_library_id: i64,
    /// Encoding status (0-10)
    pub status: i32,
    /// Video duration in seconds
    #[serde(default)]
    pub length: f64,
    /// Video width
    #[serde(default)]
    pub width: i32,
    /// Video height
    #[serde(default)]
    pub height: i32,
    /// File size in bytes
    #[serde(default)]
    pub storage_size: i64,
    /// Thumbnail filename
    pub thumbnail_file_name: Option<String>,
    /// Whether transcoding is complete
    #[serde(default)]
    pub encode_progress: i32,
    /// Available resolutions
    #[serde(default)]
    pub available_resolutions: Option<String>,
}

impl VideoInfo {
    /// Check if encoding is complete (status 3 or 4)
    pub fn is_encoding_complete(&self) -> bool {
        self.status == 3 || self.status == 4
    }

    /// Check if encoding failed (status 5)
    pub fn is_encoding_failed(&self) -> bool {
        self.status == 5
    }

    /// Get encoding progress as percentage
    pub fn encoding_progress(&self) -> i32 {
        self.encode_progress
    }
}

/// Webhook payload from Bunny Stream
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WebhookPayload {
    /// Video library ID
    pub video_library_id: i64,
    /// Video GUID
    pub video_guid: String,
    /// Status code (0-10)
    /// 0 = Queued, 1 = Processing, 2 = Encoding, 3 = Finished
    /// 4 = Resolution Finished, 5 = Failed
    /// 6 = PresignedUploadStarted, 7 = PresignedUploadFinished
    /// 8 = PresignedUploadFailed, 9 = CaptionsGenerated
    /// 10 = TitleOrDescriptionGenerated
    pub status: i32,
}

impl WebhookPayload {
    /// Check if encoding is complete
    pub fn is_finished(&self) -> bool {
        self.status == 3
    }

    /// Check if a resolution finished (video playable)
    pub fn is_resolution_finished(&self) -> bool {
        self.status == 4
    }

    /// Check if encoding failed
    pub fn is_failed(&self) -> bool {
        self.status == 5
    }

    /// Get human-readable status
    pub fn status_name(&self) -> &'static str {
        match self.status {
            0 => "Queued",
            1 => "Processing",
            2 => "Encoding",
            3 => "Finished",
            4 => "ResolutionFinished",
            5 => "Failed",
            6 => "PresignedUploadStarted",
            7 => "PresignedUploadFinished",
            8 => "PresignedUploadFailed",
            9 => "CaptionsGenerated",
            10 => "TitleOrDescriptionGenerated",
            _ => "Unknown",
        }
    }
}

/// Bunny encoding status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum BunnyStatus {
    Queued = 0,
    Processing = 1,
    Encoding = 2,
    Finished = 3,
    ResolutionFinished = 4,
    Failed = 5,
    PresignedUploadStarted = 6,
    PresignedUploadFinished = 7,
    PresignedUploadFailed = 8,
    CaptionsGenerated = 9,
    TitleOrDescriptionGenerated = 10,
}
