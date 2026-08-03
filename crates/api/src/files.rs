use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use strife_db::{ArtifactState, ArtifactType, JobType, UpsertArtifact};
use strife_storage::{StorageBackend, StorageKey};
use tokio_util::io::ReaderStream;
use tracing::{error, warn};
use uuid::Uuid;

use crate::error::ApiError;

/// Messages preserved verbatim from the ad-hoc handling this replaced.
const NOT_FOUND: &str = "File was not found";
const INTERNAL: &str = "The file request could not be completed";
const ROUTE: &str = "/api/files";

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
        .route("/api/files/{id}/text", get(file_text))
        .route("/api/files/{id}/preview-native", get(preview_native))
        .route("/api/files/{id}/preview", get(preview_request))
        .route("/api/files/{id}/thumbnail", get(thumbnail_request))
        .route("/api/files/{id}/download", get(download_file))
        .with_state(FileState { pool, storage })
}

async fn preview_request(
    State(state): State<FileState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let file = match strife_db::get_file_object_by_node_id(&state.pool, id).await {
        Ok(Some(file)) => file,
        Ok(None) => return status_response(StatusCode::NOT_FOUND),
        Err(error) => {
            error!(%error, route = ROUTE, node_id = %id, "failed to load file for preview");
            return status_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let mime = file
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    if is_native_preview_mime(mime) {
        return try_serve_original(&state, id, &headers, true)
            .await
            .unwrap_or_else(IntoResponse::into_response);
    }
    artifact_request(&state, id, ArtifactType::Preview, mime).await
}

async fn thumbnail_request(State(state): State<FileState>, Path(id): Path<Uuid>) -> Response {
    let file = match strife_db::get_file_object_by_node_id(&state.pool, id).await {
        Ok(Some(file)) => file,
        Ok(None) => return status_response(StatusCode::NOT_FOUND),
        Err(error) => {
            error!(%error, route = ROUTE, node_id = %id, "failed to load file for thumbnail");
            return status_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    artifact_request(
        &state,
        id,
        ArtifactType::Thumbnail,
        file.mime_type
            .as_deref()
            .unwrap_or("application/octet-stream"),
    )
    .await
}

async fn artifact_request(state: &FileState, id: Uuid, kind: ArtifactType, mime: &str) -> Response {
    let supported = mime.starts_with("image/")
        || mime.starts_with("video/")
        || mime == "application/pdf"
        || mime == "application/msword"
        || mime.contains("wordprocessingml");
    if !supported {
        return ApiError::PreviewNotSupported.into_response();
    }
    if let Ok(Some(artifact)) = strife_db::get_artifact(&state.pool, id, kind).await {
        if artifact.state == ArtifactState::Ready {
            if let Ok(key) = Uuid::parse_str(&artifact.storage_key) {
                if let Ok(reader) = state.storage.get_stream(StorageKey::artifact(key)).await {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, artifact.format)
                        .header(
                            header::CACHE_CONTROL,
                            "private, max-age=31536000, immutable",
                        )
                        .header("x-content-type-options", "nosniff")
                        .body(Body::from_stream(ReaderStream::new(reader)))
                        .unwrap();
                }
            }
        } else if artifact.state == ArtifactState::Generating {
            return generating_response(state, id).await;
        }
    }
    let key = Uuid::new_v4();
    let key_string = key.simple().to_string();
    let artifact = strife_db::create_or_update_artifact(
        &state.pool,
        &UpsertArtifact {
            node_id: id,
            artifact_type: kind,
            format: "application/octet-stream",
            width: None,
            height: None,
            storage_key: &key_string,
            byte_size: 0,
            generator_version: "preview-v1",
            state: ArtifactState::Generating,
        },
    )
    .await;
    if let Err(error) = artifact {
        error!(%error, route = ROUTE, node_id = %id, "failed to create generating artifact");
        return status_response(StatusCode::INTERNAL_SERVER_ERROR);
    }
    generating_response(state, id).await
}

async fn generating_response(state: &FileState, id: Uuid) -> Response {
    let job = match strife_db::enqueue_job(&state.pool, JobType::PreviewGeneration, id, 0).await {
        Ok(Some(job)) => job,
        Ok(None) => match sqlx::query_as::<_, strife_db::JobRecord>(
            "SELECT * FROM jobs WHERE target_node_id=$1 AND job_type='preview_generation' AND state IN ('pending','leased')",
        )
        .bind(id)
        .fetch_one(&state.pool)
        .await
        {
            Ok(job) => job,
            Err(error) => {
                error!(%error, route = ROUTE, node_id = %id, "failed to load pending preview job");
                return status_response(StatusCode::INTERNAL_SERVER_ERROR);
            }
        },
        Err(error) => {
            error!(%error, route = ROUTE, node_id = %id, "failed to enqueue preview generation");
            return status_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status":"generating","job_id":job.id})),
    )
        .into_response()
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

#[derive(Deserialize)]
struct TextQuery {
    after_page: Option<i32>,
    limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
struct TextPageResponse {
    page_number: i32,
    content: String,
    confidence: Option<f32>,
    width: Option<i32>,
    height: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
struct FileTextResponse {
    status: String,
    source: Option<String>,
    language: Option<String>,
    engine_name: Option<String>,
    engine_version: Option<String>,
    mean_confidence: Option<f32>,
    warnings: Vec<String>,
    pages: Vec<TextPageResponse>,
    next_page: Option<i32>,
}

async fn file_details(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<FileDetailsResponse>, ApiError> {
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
    .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?
    .ok_or(ApiError::NotFound(NOT_FOUND))?;
    details.processing_status = processing_status(&state.pool, node_id).await?;
    Ok(Json(details))
}

async fn processing_status(pool: &PgPool, node_id: Uuid) -> Result<ProcessingStatus, ApiError> {
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
        .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
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
) -> Result<Json<Vec<MetadataResponse>>, ApiError> {
    ensure_active_file(&state.pool, node_id).await?;
    let records = sqlx::query_as!(
        MetadataResponse,
        r#"
        SELECT id, extractor_name, extractor_version, status::text AS "status!",
            CASE WHEN $2 THEN raw_payload ELSE NULL END AS raw_payload,
            warnings, created_at, updated_at
        FROM metadata_records WHERE node_id = $1 ORDER BY extractor_name
        "#,
        node_id,
        query.raw
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
    Ok(Json(records))
}

async fn file_streams(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<MediaStreamResponse>>, ApiError> {
    ensure_active_file(&state.pool, node_id).await?;
    let streams = sqlx::query_as!(
        MediaStreamResponse,
        r#"
        SELECT id, stream_index, stream_type::text AS "stream_type!", codec, width, height,
            duration_ms, bitrate_bps, frame_rate, language, created_at
        FROM media_streams WHERE node_id = $1 ORDER BY stream_index
        "#,
        node_id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
    Ok(Json(streams))
}

async fn file_text(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
    Query(query): Query<TextQuery>,
) -> Result<Json<FileTextResponse>, ApiError> {
    ensure_active_file(&state.pool, node_id).await?;
    let record = strife_db::get_document_text(&state.pool, node_id)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
    let Some(record) = record else {
        let active: bool = sqlx::query_scalar!("SELECT EXISTS (SELECT 1 FROM jobs WHERE target_node_id = $1 AND job_type = 'ocr' AND state IN ('pending', 'leased')) AS \"exists!\"", node_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
        return Ok(Json(FileTextResponse {
            status: if active {
                "in_progress"
            } else {
                "not_processed"
            }
            .to_owned(),
            source: None,
            language: None,
            engine_name: None,
            engine_version: None,
            mean_confidence: None,
            warnings: Vec::new(),
            pages: Vec::new(),
            next_page: None,
        }));
    };
    let limit = query.limit.unwrap_or(25).clamp(1, 50);
    let mut pages = sqlx::query_as!(
        TextPageResponse,
        r"
        SELECT page_number, content, confidence, width, height
        FROM document_text_pages
        WHERE node_id = $1 AND page_number > $2
        ORDER BY page_number
        LIMIT $3
        ",
        node_id,
        query.after_page.unwrap_or(0),
        i64::from(limit.saturating_add(1))
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
    let has_more = pages.len() > limit as usize;
    if has_more {
        pages.pop();
    }
    let next_page = has_more
        .then(|| pages.last().map(|page| page.page_number))
        .flatten();
    let status = if record.status == strife_db::DocumentTextStatus::Completed
        && record.source == strife_db::DocumentTextSource::Embedded
    {
        "skipped_embedded".to_owned()
    } else {
        format!("{:?}", record.status).to_lowercase()
    };
    Ok(Json(FileTextResponse {
        status,
        source: Some(format!("{:?}", record.source).to_lowercase()),
        language: Some(record.language),
        engine_name: Some(record.engine_name),
        engine_version: Some(record.engine_version),
        mean_confidence: record.mean_confidence,
        warnings: record.warnings,
        pages,
        next_page,
    }))
}

async fn ensure_active_file(pool: &PgPool, node_id: Uuid) -> Result<(), ApiError> {
    let exists: bool = sqlx::query_scalar!("SELECT EXISTS (SELECT 1 FROM nodes WHERE id = $1 AND kind = 'file' AND lifecycle_state = 'active') AS \"exists!\"", node_id)
    .fetch_one(pool)
    .await
    .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::NotFound(NOT_FOUND))
    }
}

async fn download_file(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    try_serve_original(&state, node_id, &headers, false)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

async fn preview_native(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    try_serve_original(&state, node_id, &headers, true)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

async fn try_serve_original(
    state: &FileState,
    node_id: Uuid,
    headers: &HeaderMap,
    inline: bool,
) -> Result<Response, ApiError> {
    let file = strife_db::get_download_file(&state.pool, node_id)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?
        .ok_or(ApiError::NotFound(NOT_FOUND))?;
    let object_id = Uuid::parse_str(&file.storage_key)
        .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
    let mime = file
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    if inline && !is_native_preview_mime(mime) {
        return Err(ApiError::NotFound(NOT_FOUND));
    }
    let total = u64::try_from(file.byte_size)
        .map_err(|error| ApiError::internal_with(error, ROUTE, node_id, INTERNAL))?;
    let requested_range = headers
        .get(header::RANGE)
        .map(|value| parse_range(value, total).map_err(|()| ApiError::RangeNotSatisfiable(total)))
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
    // A missing object is a 404 to the caller, but the cause matters: a
    // permissions or device failure looks identical from outside and would
    // otherwise be invisible.
    .map_err(|error| {
        warn!(%error, node_id = %node_id, route = ROUTE, "original bytes were unreadable");
        ApiError::NotFound(NOT_FOUND)
    })?;
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
    Ok(response.body(body).unwrap_or_else(|error| {
        ApiError::internal_with(error, ROUTE, node_id, INTERNAL).into_response()
    }))
}

/// Types the browser may render inline in Strife's own origin.
///
/// SVG is excluded even though it is an image type: it is a document that can
/// carry script, so serving one inline would execute attacker-supplied code in
/// the application origin. `nosniff` does not help here, because the type is
/// declared correctly — the type is simply not safe to render. The same applies
/// to the XML-ish image types below.
///
/// Shared with the attachment endpoint so the archive and the file browser
/// cannot drift into two different answers about what is safe to render.
pub(crate) fn is_native_preview_mime(mime: &str) -> bool {
    let base = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();
    if matches!(
        base.as_str(),
        "image/svg+xml" | "image/svg" | "image/x-svg" | "text/html" | "application/xhtml+xml"
    ) {
        return false;
    }
    base.starts_with("image/")
        || base.starts_with("video/")
        || base.starts_with("audio/")
        || base == "application/pdf"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ByteRange {
    pub(crate) start: u64,
    pub(crate) end: u64,
}

impl ByteRange {
    pub(crate) const fn length(self) -> u64 {
        self.end - self.start + 1
    }
}

pub(crate) fn parse_range(value: &HeaderValue, total: u64) -> Result<ByteRange, ()> {
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

/// Renders a name safe to place in a `Content-Disposition` header.
///
/// Two separate concerns. Quotes, backslashes, and control characters would let
/// a sender-chosen name close the quoted parameter and append headers of their
/// own. Path separators are stripped because the browser uses this value to name
/// the saved file, and a name like `../../etc/passwd` should arrive as
/// `passwd` rather than as something a download manager might interpret.
pub(crate) fn safe_filename(name: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = leaf
        .chars()
        .map(|character| match character {
            '"' | '\\' | '/' => '_',
            control if control.is_control() => '_',
            other => other,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "attachment".to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(crate) fn range_not_satisfiable(total: u64) -> Response {
    ApiError::RangeNotSatisfiable(total).into_response()
}

pub(crate) fn status_response(status: StatusCode) -> Response {
    match status {
        StatusCode::NOT_FOUND => ApiError::NotFound(NOT_FOUND),
        _ => ApiError::Internal(INTERNAL),
    }
    .into_response()
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
