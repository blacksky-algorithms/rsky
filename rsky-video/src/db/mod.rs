//! Database operations for video jobs and quotas

use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tracing::info;
use uuid::Uuid;

use crate::error::{Error, Result};

/// Video job record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoJob {
    pub id: i64,
    pub job_id: Uuid,
    pub did: String,
    pub bunny_video_id: Option<String>,
    pub state: String,
    pub progress: i32,
    pub blob_ref: Option<JsonValue>,
    pub error: Option<String>,
    pub message: Option<String>,
    pub original_filename: Option<String>,
    pub file_size: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Job state constants
pub mod job_state {
    pub const CREATED: &str = "JOB_STATE_CREATED";
    pub const UPLOADING: &str = "JOB_STATE_UPLOADING";
    pub const PROCESSING: &str = "JOB_STATE_PROCESSING";
    pub const COMPLETED: &str = "JOB_STATE_COMPLETED";
    pub const FAILED: &str = "JOB_STATE_FAILED";
}

/// Upload quota record
#[derive(Debug, Clone)]
pub struct UploadQuota {
    pub did: String,
    pub daily_videos_used: i32,
    pub daily_bytes_used: i64,
    pub quota_reset_at: DateTime<Utc>,
}

/// Run database migrations
pub async fn run_migrations(pool: &Pool) -> Result<()> {
    let client = pool.get().await?;

    // Create video_jobs table
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS video_jobs (
                id BIGSERIAL PRIMARY KEY,
                job_id UUID NOT NULL UNIQUE,
                did TEXT NOT NULL,
                bunny_video_id TEXT,
                state TEXT NOT NULL DEFAULT 'JOB_STATE_CREATED',
                progress INTEGER DEFAULT 0,
                blob_ref JSONB,
                error TEXT,
                message TEXT,
                original_filename TEXT,
                file_size BIGINT,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                updated_at TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
            &[],
        )
        .await?;

    // Create index on job_id
    client
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_video_jobs_job_id ON video_jobs (job_id)",
            &[],
        )
        .await?;

    // Create index on bunny_video_id for webhook lookups
    client
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_video_jobs_bunny_video_id ON video_jobs (bunny_video_id)",
            &[],
        )
        .await?;

    // Create index on did for quota lookups
    client
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_video_jobs_did ON video_jobs (did)",
            &[],
        )
        .await?;

    // Create upload_quotas table
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS upload_quotas (
                did TEXT PRIMARY KEY,
                daily_videos_used INTEGER DEFAULT 0,
                daily_bytes_used BIGINT DEFAULT 0,
                quota_reset_at TIMESTAMPTZ DEFAULT NOW(),
                created_at TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
            &[],
        )
        .await?;

    // Create video_mappings table for did/cid -> bunny_video_id mapping
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS video_mappings (
                id BIGSERIAL PRIMARY KEY,
                did TEXT NOT NULL,
                cid TEXT NOT NULL,
                bunny_video_id TEXT NOT NULL,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                UNIQUE(did, cid)
            )
            "#,
            &[],
        )
        .await?;

    // Create index for video mapping lookups
    client
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_video_mappings_did_cid ON video_mappings (did, cid)",
            &[],
        )
        .await?;

    info!("Database migrations completed");
    Ok(())
}

/// Create a new video job
pub async fn create_job(
    pool: &Pool,
    did: &str,
    filename: Option<&str>,
    file_size: Option<i64>,
) -> Result<VideoJob> {
    let client = pool.get().await?;
    let job_id = Uuid::new_v4();

    let row = client
        .query_one(
            r#"
            INSERT INTO video_jobs (job_id, did, original_filename, file_size)
            VALUES ($1, $2, $3, $4)
            RETURNING id, job_id, did, bunny_video_id, state, progress, blob_ref, error, message, original_filename, file_size, created_at, updated_at
            "#,
            &[&job_id, &did, &filename, &file_size],
        )
        .await?;

    Ok(row_to_job(&row))
}

/// Get a job by job_id
pub async fn get_job(pool: &Pool, job_id: Uuid) -> Result<Option<VideoJob>> {
    let client = pool.get().await?;

    let row = client
        .query_opt(
            r#"
            SELECT id, job_id, did, bunny_video_id, state, progress, blob_ref, error, message, original_filename, file_size, created_at, updated_at
            FROM video_jobs
            WHERE job_id = $1
            "#,
            &[&job_id],
        )
        .await?;

    Ok(row.map(|r| row_to_job(&r)))
}

/// Get a job by bunny_video_id (for webhook handling)
pub async fn get_job_by_bunny_id(pool: &Pool, bunny_video_id: &str) -> Result<Option<VideoJob>> {
    let client = pool.get().await?;

    let row = client
        .query_opt(
            r#"
            SELECT id, job_id, did, bunny_video_id, state, progress, blob_ref, error, message, original_filename, file_size, created_at, updated_at
            FROM video_jobs
            WHERE bunny_video_id = $1
            "#,
            &[&bunny_video_id],
        )
        .await?;

    Ok(row.map(|r| row_to_job(&r)))
}

/// Update job with bunny video ID
pub async fn set_bunny_video_id(pool: &Pool, job_id: Uuid, bunny_video_id: &str) -> Result<()> {
    let client = pool.get().await?;

    client
        .execute(
            r#"
            UPDATE video_jobs
            SET bunny_video_id = $2, state = 'JOB_STATE_UPLOADING', updated_at = NOW()
            WHERE job_id = $1
            "#,
            &[&job_id, &bunny_video_id],
        )
        .await?;

    Ok(())
}

/// Update job state
pub async fn update_job_state(pool: &Pool, job_id: Uuid, state: &str, progress: i32) -> Result<()> {
    let client = pool.get().await?;

    client
        .execute(
            r#"
            UPDATE video_jobs
            SET state = $2, progress = $3, updated_at = NOW()
            WHERE job_id = $1
            "#,
            &[&job_id, &state, &progress],
        )
        .await?;

    Ok(())
}

/// Mark job as completed with blob ref
pub async fn complete_job(pool: &Pool, job_id: Uuid, blob_ref: JsonValue) -> Result<()> {
    let client = pool.get().await?;

    client
        .execute(
            r#"
            UPDATE video_jobs
            SET state = 'JOB_STATE_COMPLETED', progress = 100, blob_ref = $2, updated_at = NOW()
            WHERE job_id = $1
            "#,
            &[&job_id, &blob_ref],
        )
        .await?;

    Ok(())
}

/// Mark job as failed
pub async fn fail_job(pool: &Pool, job_id: Uuid, error: &str) -> Result<()> {
    let client = pool.get().await?;

    client
        .execute(
            r#"
            UPDATE video_jobs
            SET state = 'JOB_STATE_FAILED', error = $2, updated_at = NOW()
            WHERE job_id = $1
            "#,
            &[&job_id, &error],
        )
        .await?;

    Ok(())
}

/// Get or create upload quota for a user
pub async fn get_or_create_quota(pool: &Pool, did: &str) -> Result<UploadQuota> {
    let client = pool.get().await?;
    let now = Utc::now();

    // Try to get existing quota
    let row = client
        .query_opt(
            "SELECT did, daily_videos_used, daily_bytes_used, quota_reset_at FROM upload_quotas WHERE did = $1",
            &[&did],
        )
        .await?;

    if let Some(row) = row {
        let quota_reset_at: DateTime<Utc> = row.get(3);

        // Check if quota should be reset (new day)
        if now.date_naive() > quota_reset_at.date_naive() {
            // Reset quota
            client
                .execute(
                    "UPDATE upload_quotas SET daily_videos_used = 0, daily_bytes_used = 0, quota_reset_at = $2 WHERE did = $1",
                    &[&did, &now],
                )
                .await?;

            return Ok(UploadQuota {
                did: did.to_string(),
                daily_videos_used: 0,
                daily_bytes_used: 0,
                quota_reset_at: now,
            });
        }

        return Ok(UploadQuota {
            did: row.get(0),
            daily_videos_used: row.get(1),
            daily_bytes_used: row.get(2),
            quota_reset_at,
        });
    }

    // Create new quota record
    client
        .execute(
            "INSERT INTO upload_quotas (did, quota_reset_at) VALUES ($1, $2) ON CONFLICT (did) DO NOTHING",
            &[&did, &now],
        )
        .await?;

    Ok(UploadQuota {
        did: did.to_string(),
        daily_videos_used: 0,
        daily_bytes_used: 0,
        quota_reset_at: now,
    })
}

/// Increment quota usage
pub async fn increment_quota(pool: &Pool, did: &str, bytes: i64) -> Result<()> {
    let client = pool.get().await?;

    client
        .execute(
            "UPDATE upload_quotas SET daily_videos_used = daily_videos_used + 1, daily_bytes_used = daily_bytes_used + $2 WHERE did = $1",
            &[&did, &bytes],
        )
        .await?;

    Ok(())
}

/// Save video mapping (did/cid -> bunny_video_id)
pub async fn save_video_mapping(
    pool: &Pool,
    did: &str,
    cid: &str,
    bunny_video_id: &str,
) -> Result<()> {
    let client = pool.get().await?;

    client
        .execute(
            r#"
            INSERT INTO video_mappings (did, cid, bunny_video_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (did, cid) DO UPDATE SET bunny_video_id = $3
            "#,
            &[&did, &cid, &bunny_video_id],
        )
        .await?;

    Ok(())
}

/// Get bunny video ID from did/cid mapping
pub async fn get_bunny_video_id(pool: &Pool, did: &str, cid: &str) -> Result<Option<String>> {
    let client = pool.get().await?;

    let row = client
        .query_opt(
            "SELECT bunny_video_id FROM video_mappings WHERE did = $1 AND cid = $2",
            &[&did, &cid],
        )
        .await?;

    Ok(row.map(|r| r.get(0)))
}

fn row_to_job(row: &tokio_postgres::Row) -> VideoJob {
    VideoJob {
        id: row.get(0),
        job_id: row.get(1),
        did: row.get(2),
        bunny_video_id: row.get(3),
        state: row.get(4),
        progress: row.get(5),
        blob_ref: row.get(6),
        error: row.get(7),
        message: row.get(8),
        original_filename: row.get(9),
        file_size: row.get(10),
        created_at: row.get(11),
        updated_at: row.get(12),
    }
}
