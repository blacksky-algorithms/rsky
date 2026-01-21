//! XRPC endpoint handlers for app.bsky.video.* methods

use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::{
    AppState, auth,
    bunny::WebhookPayload,
    db::{self, job_state},
    error::{Error, Result},
};

/// Query parameters for getUploadLimits
#[derive(Debug, Deserialize)]
pub struct GetUploadLimitsParams {}

/// Response for getUploadLimits
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetUploadLimitsResponse {
    pub can_upload: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_daily_videos: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_daily_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// GET /xrpc/app.bsky.video.getUploadLimits
pub async fn get_upload_limits(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<GetUploadLimitsResponse>> {
    // Validate service auth
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let user_did = auth::get_user_did(auth_header, &state.config.service_did)?;
    debug!("getUploadLimits for user: {}", user_did);

    // Get user's quota
    let quota = db::get_or_create_quota(&state.db_pool, &user_did).await?;

    let remaining_videos = state.config.daily_video_limit as i32 - quota.daily_videos_used;
    let remaining_bytes = state.config.daily_byte_limit as i64 - quota.daily_bytes_used;

    // Check if user can upload
    let can_upload = remaining_videos > 0 && remaining_bytes > 0;

    let response = if can_upload {
        GetUploadLimitsResponse {
            can_upload: true,
            remaining_daily_videos: Some(remaining_videos),
            remaining_daily_bytes: Some(remaining_bytes),
            message: None,
            error: None,
        }
    } else if remaining_videos <= 0 {
        GetUploadLimitsResponse {
            can_upload: false,
            remaining_daily_videos: Some(0),
            remaining_daily_bytes: Some(remaining_bytes),
            message: Some("User has exceeded daily upload videos limit".to_string()),
            error: None,
        }
    } else {
        GetUploadLimitsResponse {
            can_upload: false,
            remaining_daily_videos: Some(remaining_videos),
            remaining_daily_bytes: Some(0),
            message: Some("User has exceeded daily upload bytes limit".to_string()),
            error: None,
        }
    };

    Ok(Json(response))
}

/// Query parameters for uploadVideo
#[derive(Debug, Deserialize)]
pub struct UploadVideoParams {
    pub did: String,
    pub name: String,
}

/// Response for uploadVideo
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadVideoResponse {
    pub job_status: JobStatus,
}

/// Job status in API responses
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub job_id: String,
    pub did: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// POST /xrpc/app.bsky.video.uploadVideo
pub async fn upload_video(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<UploadVideoParams>,
    body: Bytes,
) -> Result<Json<JobStatus>> {
    // Validate service auth
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    // The token should be for com.atproto.repo.uploadBlob
    let token = auth::extract_auth_header(auth_header)?;
    let claims = auth::validate_service_auth(&token, &state.config.service_did, None)?;

    // Verify the DID matches
    if claims.user_did() != params.did {
        return Err(Error::Forbidden(
            "Token subject does not match upload DID".to_string(),
        ));
    }

    let user_did = &params.did;
    let file_size = body.len() as i64;

    info!(
        "uploadVideo: did={}, name={}, size={}",
        user_did, params.name, file_size
    );

    // Check file size
    if file_size > state.config.max_video_size as i64 {
        return Err(Error::VideoTooLarge(format!(
            "file size ({} bytes) is larger than the maximum allowed size ({} bytes)",
            file_size, state.config.max_video_size
        )));
    }

    // Check quota
    let quota = db::get_or_create_quota(&state.db_pool, user_did).await?;
    let remaining_videos = state.config.daily_video_limit as i32 - quota.daily_videos_used;
    let remaining_bytes = state.config.daily_byte_limit as i64 - quota.daily_bytes_used;

    if remaining_videos <= 0 {
        return Err(Error::UploadLimitExceeded(
            "User has exceeded daily upload videos limit".to_string(),
        ));
    }
    if remaining_bytes < file_size {
        return Err(Error::UploadLimitExceeded(
            "User has exceeded daily upload bytes limit".to_string(),
        ));
    }

    // Create job in database
    let job = db::create_job(
        &state.db_pool,
        user_did,
        Some(&params.name),
        Some(file_size),
    )
    .await?;

    let job_id = job.job_id;
    info!("Created job: {}", job_id);

    // Create video in Bunny Stream
    let title = format!("{}_{}", user_did, params.name);
    let bunny_video = match state.bunny_client.create_video(&title).await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to create Bunny video: {}", e);
            db::fail_job(&state.db_pool, job_id, &e.to_string()).await?;
            return Err(e);
        }
    };

    let bunny_video_id = bunny_video.guid.clone();
    db::set_bunny_video_id(&state.db_pool, job_id, &bunny_video_id).await?;

    // Upload video to Bunny
    if let Err(e) = state.bunny_client.upload_video(&bunny_video_id, body).await {
        error!("Failed to upload to Bunny: {}", e);
        db::fail_job(&state.db_pool, job_id, &e.to_string()).await?;
        return Err(e);
    }

    // Update job state to processing
    db::update_job_state(&state.db_pool, job_id, job_state::PROCESSING, 0).await?;

    // Increment quota
    db::increment_quota(&state.db_pool, user_did, file_size).await?;

    info!(
        "Video uploaded to Bunny: job={}, bunny_id={}",
        job_id, bunny_video_id
    );

    // Return flat JobStatus (not wrapped) - client expects this format
    Ok(Json(JobStatus {
        job_id: job_id.to_string(),
        did: user_did.to_string(),
        state: job_state::PROCESSING.to_string(),
        progress: Some(0),
        blob: None,
        error: None,
        message: Some("Video is being processed".to_string()),
    }))
}

/// Query parameters for getJobStatus
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetJobStatusParams {
    pub job_id: String,
}

/// Response for getJobStatus
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetJobStatusResponse {
    pub job_status: JobStatus,
}

/// GET /xrpc/app.bsky.video.getJobStatus
pub async fn get_job_status(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GetJobStatusParams>,
) -> Result<Json<JobStatus>> {
    let job_id = Uuid::parse_str(&params.job_id)
        .map_err(|_| Error::BadRequest("Invalid job ID format".to_string()))?;

    let job = db::get_job(&state.db_pool, job_id)
        .await?
        .ok_or_else(|| Error::NotFound("Job not found".to_string()))?;

    // If job is still processing, check Bunny status
    let (state_str, progress) = if job.state == job_state::PROCESSING {
        if let Some(bunny_id) = &job.bunny_video_id {
            match state.bunny_client.get_video(bunny_id).await {
                Ok(video_info) => {
                    if video_info.is_encoding_complete() {
                        (job_state::PROCESSING.to_string(), 99)
                    } else if video_info.is_encoding_failed() {
                        (job_state::FAILED.to_string(), job.progress)
                    } else {
                        (job.state.clone(), video_info.encoding_progress())
                    }
                }
                Err(e) => {
                    warn!("Failed to get Bunny video status: {}", e);
                    (job.state.clone(), job.progress)
                }
            }
        } else {
            (job.state.clone(), job.progress)
        }
    } else {
        (job.state.clone(), job.progress)
    };

    // Return flat JobStatus (not wrapped) - client expects this format
    Ok(Json(JobStatus {
        job_id: job.job_id.to_string(),
        did: job.did,
        state: state_str,
        progress: Some(progress),
        blob: job.blob_ref,
        error: job.error,
        message: job.message,
    }))
}

/// POST /webhook/bunny - Handle Bunny Stream webhook callbacks
pub async fn bunny_webhook(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WebhookPayload>,
) -> Result<StatusCode> {
    info!(
        "Bunny webhook: video={}, status={} ({})",
        payload.video_guid,
        payload.status,
        payload.status_name()
    );

    // Find the job by bunny video ID
    let job = match db::get_job_by_bunny_id(&state.db_pool, &payload.video_guid).await? {
        Some(j) => j,
        None => {
            warn!("Webhook for unknown video: {}", payload.video_guid);
            return Ok(StatusCode::OK);
        }
    };

    if payload.is_finished() || payload.is_resolution_finished() {
        // Video encoding is complete
        info!("Video encoding complete: job={}", job.job_id);

        // Get video info from Bunny
        let video_info = state.bunny_client.get_video(&payload.video_guid).await?;

        // Create a blob ref that points to the video
        // In a full implementation, we'd upload to the user's PDS here
        // For MVP, we create a synthetic blob ref
        let blob_ref = json!({
            "$type": "blob",
            "ref": {
                "$link": payload.video_guid
            },
            "mimeType": "video/mp4",
            "size": video_info.storage_size
        });

        // Save the mapping for URL proxy
        // The CID is the bunny video ID for now
        db::save_video_mapping(
            &state.db_pool,
            &job.did,
            &payload.video_guid,
            &payload.video_guid,
        )
        .await?;

        // Mark job as complete
        db::complete_job(&state.db_pool, job.job_id, blob_ref).await?;

        info!("Job completed: {}", job.job_id);
    } else if payload.is_failed() {
        // Video encoding failed
        error!("Video encoding failed: job={}", job.job_id);
        db::fail_job(&state.db_pool, job.job_id, "Video encoding failed").await?;
    } else {
        // Update progress
        let progress = match payload.status {
            0 => 0,  // Queued
            1 => 10, // Processing
            2 => 50, // Encoding
            _ => job.progress,
        };
        db::update_job_state(&state.db_pool, job.job_id, job_state::PROCESSING, progress).await?;
    }

    Ok(StatusCode::OK)
}

/// Path parameters for video proxy
#[derive(Debug, Deserialize)]
pub struct VideoProxyPath {
    pub did: String,
    pub cid: String,
}

/// GET /stream/:did/:cid/playlist.m3u8 - Proxy HLS playlist
pub async fn proxy_playlist(
    State(state): State<Arc<AppState>>,
    Path(path): Path<VideoProxyPath>,
) -> Result<Response> {
    let did = urlencoding::decode(&path.did)
        .map_err(|_| Error::BadRequest("Invalid DID encoding".to_string()))?;
    let cid = urlencoding::decode(&path.cid)
        .map_err(|_| Error::BadRequest("Invalid CID encoding".to_string()))?;

    debug!("Proxy playlist: did={}, cid={}", did, cid);

    // Look up the bunny video ID in our database
    let redirect_url = match db::get_bunny_video_id(&state.db_pool, &did, &cid).await? {
        Some(bunny_video_id) => {
            // Video is in our system - redirect to Bunny CDN
            state.bunny_client.get_playlist_url(&bunny_video_id)
        }
        None => {
            // Video not in our system - fallback to Bluesky's video CDN
            debug!("Video not in our DB, falling back to Bluesky CDN: did={}, cid={}", did, cid);
            format!("https://video.bsky.app/watch/{}/{}/playlist.m3u8", did, cid)
        }
    };

    Ok(Response::builder()
        .status(StatusCode::TEMPORARY_REDIRECT)
        .header(header::LOCATION, redirect_url)
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::empty())
        .unwrap())
}

/// GET /stream/:did/:cid/thumbnail.jpg - Proxy thumbnail
pub async fn proxy_thumbnail(
    State(state): State<Arc<AppState>>,
    Path(path): Path<VideoProxyPath>,
) -> Result<Response> {
    let did = urlencoding::decode(&path.did)
        .map_err(|_| Error::BadRequest("Invalid DID encoding".to_string()))?;
    let cid = urlencoding::decode(&path.cid)
        .map_err(|_| Error::BadRequest("Invalid CID encoding".to_string()))?;

    debug!("Proxy thumbnail: did={}, cid={}", did, cid);

    // Look up the bunny video ID in our database
    let redirect_url = match db::get_bunny_video_id(&state.db_pool, &did, &cid).await? {
        Some(bunny_video_id) => {
            // Video is in our system - redirect to Bunny CDN
            state.bunny_client.get_thumbnail_url(&bunny_video_id)
        }
        None => {
            // Video not in our system - fallback to Bluesky's video CDN
            debug!("Video not in our DB, falling back to Bluesky CDN: did={}, cid={}", did, cid);
            format!("https://video.bsky.app/watch/{}/{}/thumbnail.jpg", did, cid)
        }
    };

    Ok(Response::builder()
        .status(StatusCode::TEMPORARY_REDIRECT)
        .header(header::LOCATION, redirect_url)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::empty())
        .unwrap())
}
