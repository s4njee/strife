use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use strife_storage::{StorageBackend, StorageKey};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

#[derive(Clone)]
struct FileState {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
}

/// Builds the original-file download router.
pub fn router(pool: PgPool, storage: Arc<dyn StorageBackend>) -> Router {
    Router::new()
        .route("/api/files/{id}", get(file_details))
        .route("/api/files/{id}/metadata", get(file_metadata))
        .route("/api/files/{id}/streams", get(file_streams))
        .route("/api/files/{id}/preview-native", get(preview_native))
        .route("/api/files/{id}/download", get(download_file))
        .with_state(FileState { pool, storage })
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct FileDetailsResponse {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub byte_size: i64,
    pub checksum_sha256: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub detected_mime: Option<String>,
    pub media_kind: Option<String>,
    pub duration_ms: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub capture_time: Option<DateTime<Utc>>,
    pub page_count: Option<i32>,
    pub orientation: Option<i32>,
    pub has_gps: Option<bool>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub document_title: Option<String>,
    pub document_author: Option<String>,
    pub document_created_at: Option<DateTime<Utc>>,
    pub document_modified_at: Option<DateTime<Utc>>,
    #[sqlx(skip)]
    pub processing_status: ProcessingStatus,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingStatus {
    #[default]
    Processing,
    Ready,
    PartiallyProcessed,
    Failed,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct MetadataResponse {
    pub id: Uuid,
    pub extractor_name: String,
    pub extractor_version: String,
    pub status: String,
    pub raw_payload: Option<Value>,
    pub warnings: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct MediaStreamResponse {
    pub id: Uuid,
    pub stream_index: i32,
    pub stream_type: String,
    pub codec: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i64>,
    pub bitrate_bps: Option<i64>,
    pub frame_rate: Option<String>,
    pub language: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct MetadataQuery {
    #[serde(default)]
    raw: bool,
}

async fn file_details(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<FileDetailsResponse>, StatusCode> {
    let mut details = sqlx::query_as::<_, FileDetailsResponse>(
        r"
        SELECT n.id, n.parent_id, n.name, f.byte_size, f.checksum_sha256,
            n.created_at, n.updated_at, m.detected_mime, m.media_kind::text AS media_kind,
            m.duration_ms, m.width, m.height, m.capture_time, m.page_count, m.orientation,
            m.has_gps, m.gps_latitude, m.gps_longitude, m.camera_make, m.camera_model,
            m.document_title, m.document_author, m.document_created_at, m.document_modified_at
        FROM nodes n
        JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
        LEFT JOIN node_metadata m ON m.node_id = n.id
        WHERE n.id = $1 AND n.kind = 'file' AND n.lifecycle_state = 'active'
        ",
    )
    .bind(node_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    details.processing_status = processing_status(&state.pool, node_id).await?;
    Ok(Json(details))
}

async fn processing_status(pool: &PgPool, node_id: Uuid) -> Result<ProcessingStatus, StatusCode> {
    let (active, failed_jobs, successful, failed_metadata): (i64, i64, i64, i64) =
        sqlx::query_as(
            r"
            SELECT
                count(*) FILTER (WHERE source = 'job' AND state IN ('pending', 'leased')),
                count(*) FILTER (WHERE source = 'job' AND state = 'failed'),
                count(*) FILTER (WHERE source = 'metadata' AND state IN ('completed', 'unsupported')),
                count(*) FILTER (WHERE source = 'metadata' AND state = 'failed')
            FROM (
                SELECT 'job' AS source, state::text AS state FROM jobs WHERE target_node_id = $1
                UNION ALL
                SELECT 'metadata', status::text FROM metadata_records WHERE node_id = $1
            ) states
            ",
        )
        .bind(node_id)
        .fetch_one(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(if active > 0 {
        ProcessingStatus::Processing
    } else if successful > 0 && failed_metadata > 0 {
        ProcessingStatus::PartiallyProcessed
    } else if failed_jobs > 0 || failed_metadata > 0 {
        ProcessingStatus::Failed
    } else if successful > 0 {
        ProcessingStatus::Ready
    } else {
        ProcessingStatus::Processing
    })
}

async fn file_metadata(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
    Query(query): Query<MetadataQuery>,
) -> Result<Json<Vec<MetadataResponse>>, StatusCode> {
    ensure_active_file(&state.pool, node_id).await?;
    let records = sqlx::query_as::<_, MetadataResponse>(
        r"
        SELECT id, extractor_name, extractor_version, status::text AS status,
            CASE WHEN $2 THEN raw_payload ELSE NULL END AS raw_payload,
            warnings, created_at, updated_at
        FROM metadata_records WHERE node_id = $1 ORDER BY extractor_name
        ",
    )
    .bind(node_id)
    .bind(query.raw)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(records))
}

async fn file_streams(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<MediaStreamResponse>>, StatusCode> {
    ensure_active_file(&state.pool, node_id).await?;
    let streams = sqlx::query_as::<_, MediaStreamResponse>(
        r"
        SELECT id, stream_index, stream_type::text AS stream_type, codec, width, height,
            duration_ms, bitrate_bps, frame_rate, language, created_at
        FROM media_streams WHERE node_id = $1 ORDER BY stream_index
        ",
    )
    .bind(node_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(streams))
}

async fn ensure_active_file(pool: &PgPool, node_id: Uuid) -> Result<(), StatusCode> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM nodes WHERE id = $1 AND kind = 'file' AND lifecycle_state = 'active')",
    )
    .bind(node_id)
    .fetch_one(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists {
        Ok(())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn download_file(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    try_serve_original(&state, node_id, &headers, false)
        .await
        .unwrap_or_else(DownloadError::into_response)
}

async fn preview_native(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    try_serve_original(&state, node_id, &headers, true)
        .await
        .unwrap_or_else(DownloadError::into_response)
}

async fn try_serve_original(
    state: &FileState,
    node_id: Uuid,
    headers: &HeaderMap,
    inline: bool,
) -> Result<Response, DownloadError> {
    let file = strife_db::get_download_file(&state.pool, node_id)
        .await
        .map_err(|_| DownloadError::Internal)?
        .ok_or(DownloadError::NotFound)?;
    let object_id = Uuid::parse_str(&file.storage_key).map_err(|_| DownloadError::Internal)?;
    let mime = file
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    if inline && !is_native_preview_mime(mime) {
        return Err(DownloadError::NotFound);
    }
    let total = u64::try_from(file.byte_size).map_err(|_| DownloadError::Internal)?;
    let requested_range = headers
        .get(header::RANGE)
        .map(|value| parse_range(value, total).map_err(|()| DownloadError::Range(total)))
        .transpose()?;
    let (status, start, length) = requested_range.map_or((StatusCode::OK, 0, total), |range| {
        (StatusCode::PARTIAL_CONTENT, range.start, range.length())
    });
    let reader = if requested_range.is_some() {
        state
            .storage
            .get_range(StorageKey::original(object_id), start, length)
            .await
    } else {
        state
            .storage
            .get_stream(StorageKey::original(object_id))
            .await
    }
    .map_err(|_| DownloadError::NotFound)?;
    let body = Body::from_stream(ReaderStream::new(reader));
    let mut response = Response::builder()
        .status(status)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string())
        .header(header::CONTENT_TYPE, mime)
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "{}; filename=\"{}\"",
                if inline { "inline" } else { "attachment" },
                safe_filename(&file.display_name)
            ),
        )
        .header("x-content-type-options", "nosniff");
    if inline {
        response = response.header(header::CACHE_CONTROL, "private, max-age=3600");
    }
    if let Some(range) = requested_range {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", range.start, range.end, total),
        );
    }
    Ok(response
        .body(body)
        .unwrap_or_else(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR)))
}

fn is_native_preview_mime(mime: &str) -> bool {
    mime.starts_with("image/")
        || mime.starts_with("video/")
        || mime.starts_with("audio/")
        || mime == "application/pdf"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownloadError {
    NotFound,
    Range(u64),
    Internal,
}

impl DownloadError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => status_response(StatusCode::NOT_FOUND),
            Self::Range(total) => range_not_satisfiable(total),
            Self::Internal => status_response(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ByteRange {
    start: u64,
    end: u64,
}

impl ByteRange {
    const fn length(self) -> u64 {
        self.end - self.start + 1
    }
}

fn parse_range(value: &HeaderValue, total: u64) -> Result<ByteRange, ()> {
    let value = value
        .to_str()
        .ok()
        .and_then(|value| value.strip_prefix("bytes="))
        .filter(|value| !value.contains(','))
        .ok_or(())?;
    let (start, end) = value.split_once('-').ok_or(())?;
    let range = if start.is_empty() {
        let suffix = end
            .parse::<u64>()
            .ok()
            .filter(|suffix| *suffix > 0)
            .ok_or(())?;
        ByteRange {
            start: total.saturating_sub(suffix.min(total)),
            end: total.saturating_sub(1),
        }
    } else {
        let start = start.parse::<u64>().map_err(|_| ())?;
        let end = if end.is_empty() {
            total.saturating_sub(1)
        } else {
            end.parse::<u64>()
                .map_err(|_| ())?
                .min(total.saturating_sub(1))
        };
        ByteRange { start, end }
    };
    if total == 0 || range.start > range.end || range.start >= total {
        return Err(());
    }
    Ok(range)
}

fn safe_filename(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            '\r' | '\n' | '\0' | '"' | '\\' => '_',
            other => other,
        })
        .collect()
}

fn range_not_satisfiable(total: u64) -> Response {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{total}"))
        .body(Body::empty())
        .unwrap_or_else(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR))
}

fn status_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::{ByteRange, is_native_preview_mime, parse_range, safe_filename};

    #[test]
    fn parses_closed_open_and_suffix_ranges() {
        assert_eq!(
            parse_range(&HeaderValue::from_static("bytes=2-5"), 10).ok(),
            Some(ByteRange { start: 2, end: 5 })
        );
        assert_eq!(
            parse_range(&HeaderValue::from_static("bytes=7-"), 10).ok(),
            Some(ByteRange { start: 7, end: 9 })
        );
        assert_eq!(
            parse_range(&HeaderValue::from_static("bytes=-3"), 10).ok(),
            Some(ByteRange { start: 7, end: 9 })
        );
    }

    #[test]
    fn rejects_invalid_or_multiple_ranges_and_sanitizes_names() {
        assert!(parse_range(&HeaderValue::from_static("bytes=10-12"), 10).is_err());
        assert!(parse_range(&HeaderValue::from_static("bytes=0-1,3-4"), 10).is_err());
        assert_eq!(safe_filename("unsafe\r\n\"name"), "unsafe___name");
    }

    #[test]
    fn permits_pdf_and_browser_native_media_only() {
        for mime in ["application/pdf", "image/gif", "video/mp4", "audio/mpeg"] {
            assert!(is_native_preview_mime(mime));
        }
        assert!(!is_native_preview_mime(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ));
    }
}
