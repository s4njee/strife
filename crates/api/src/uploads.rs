use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{patch, post},
};
use chrono::{DateTime, Duration, Utc};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use strife_db::{
    CreateUploadSession, LifecycleState, NodeKind, RecordChunkError, UploadSessionState,
};
use strife_domain::FolderRules;
use strife_storage::{StorageBackend, StorageKey};
use tokio_util::io::StreamReader;
use uuid::Uuid;

#[derive(Clone)]
struct UploadState {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    session_ttl: Duration,
    disk_guard_percent: u8,
}

/// Builds the resumable-upload API router.
pub fn router(
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    session_ttl: Duration,
    disk_guard_percent: u8,
) -> Router {
    Router::new()
        .route("/api/uploads", post(create_upload))
        .route("/api/uploads/{id}", patch(upload_chunk))
        .with_state(UploadState {
            pool,
            storage,
            session_ttl,
            disk_guard_percent,
        })
}

#[derive(Debug, Deserialize)]
struct CreateUploadRequest {
    folder_id: Uuid,
    name: String,
    size: Option<i64>,
    source_created_at: Option<DateTime<Utc>>,
    source_modified_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateUploadResponse {
    pub session_id: Uuid,
    pub staging_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UploadProgressResponse {
    pub received_bytes: i64,
    pub expected_bytes: Option<i64>,
    pub complete: bool,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_percent: Option<u64>,
}

#[derive(Debug)]
enum UploadApiError {
    BadRequest(&'static str),
    NotFound,
    NameConflict,
    RangeConflict,
    Gone,
    DiskFull(u64),
    Internal,
}

impl IntoResponse for UploadApiError {
    fn into_response(self) -> Response {
        let (status, code, message, usage_percent) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message, None),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Target folder was not found",
                None,
            ),
            Self::NameConflict => (
                StatusCode::CONFLICT,
                "name_conflict",
                "An active item or upload already has this name",
                None,
            ),
            Self::RangeConflict => (
                StatusCode::CONFLICT,
                "range_conflict",
                "This byte range has already been received",
                None,
            ),
            Self::Gone => (
                StatusCode::GONE,
                "upload_inactive",
                "The upload session is no longer active",
                None,
            ),
            Self::DiskFull(usage) => (
                StatusCode::INSUFFICIENT_STORAGE,
                "disk_full",
                "Storage does not have enough safe capacity",
                Some(usage),
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The upload session could not be created",
                None,
            ),
        };
        (
            status,
            Json(ErrorBody {
                code,
                message,
                usage_percent,
            }),
        )
            .into_response()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContentRange {
    start: i64,
    end: i64,
    total: Option<i64>,
}

async fn create_upload(
    State(state): State<UploadState>,
    Json(request): Json<CreateUploadRequest>,
) -> Result<(StatusCode, Json<CreateUploadResponse>), UploadApiError> {
    FolderRules::validate_name(&request.name)
        .map_err(|_| UploadApiError::BadRequest("Upload name cannot be empty"))?;
    if request.size.is_some_and(|size| size < 0) {
        return Err(UploadApiError::BadRequest("Upload size cannot be negative"));
    }

    let folder = strife_db::get_node_by_id(&state.pool, request.folder_id)
        .await
        .map_err(|_| UploadApiError::Internal)?
        .filter(|node| {
            node.kind == NodeKind::Folder && node.lifecycle_state == LifecycleState::Active
        })
        .ok_or(UploadApiError::NotFound)?;
    if strife_db::active_child_name_exists(&state.pool, folder.id, &request.name)
        .await
        .map_err(|_| UploadApiError::Internal)?
    {
        return Err(UploadApiError::NameConflict);
    }

    let usage = state
        .storage
        .disk_usage()
        .await
        .map_err(|_| UploadApiError::Internal)?;
    let projected_used = usage.used_bytes.saturating_add(
        request
            .size
            .and_then(|size| u64::try_from(size).ok())
            .unwrap_or_default(),
    );
    let usage_percent = projected_used
        .saturating_mul(100)
        .checked_div(usage.total_bytes)
        .unwrap_or(100);
    if usage.total_bytes == 0
        || projected_used.saturating_mul(100)
            >= usage
                .total_bytes
                .saturating_mul(u64::from(state.disk_guard_percent))
    {
        return Err(UploadApiError::DiskFull(usage_percent));
    }

    let staging_key = Uuid::new_v4();
    let session = strife_db::create_session(
        &state.pool,
        CreateUploadSession {
            target_folder_id: folder.id,
            display_name: &request.name,
            expected_byte_size: request.size,
            staging_key,
            source_created_at: request.source_created_at,
            source_modified_at: request.source_modified_at,
            expires_at: Utc::now() + state.session_ttl,
        },
    )
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            UploadApiError::NameConflict
        } else {
            UploadApiError::Internal
        }
    })?;

    Ok((
        StatusCode::CREATED,
        Json(CreateUploadResponse {
            session_id: session.id,
            staging_key,
        }),
    ))
}

async fn upload_chunk(
    State(state): State<UploadState>,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<UploadProgressResponse>, UploadApiError> {
    let content_range = headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)
        .ok_or(UploadApiError::BadRequest(
            "A valid Content-Range header is required",
        ))?;
    let progress = strife_db::get_session_progress(&state.pool, session_id)
        .await
        .map_err(|_| UploadApiError::Internal)?
        .ok_or(UploadApiError::NotFound)?;
    if progress.session.state != UploadSessionState::Active {
        return Err(UploadApiError::Gone);
    }
    if content_range.total.is_some() && content_range.total != progress.session.expected_byte_size {
        return Err(UploadApiError::BadRequest(
            "Content-Range total does not match the upload session",
        ));
    }
    if progress
        .received_ranges
        .iter()
        .any(|range| range.start_byte <= content_range.end && range.end_byte >= content_range.start)
    {
        return Err(UploadApiError::RangeConflict);
    }

    let staging_id =
        Uuid::parse_str(&progress.session.staging_key).map_err(|_| UploadApiError::Internal)?;
    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let reader = StreamReader::new(stream);
    let written = state
        .storage
        .write_range(
            StorageKey::staging(staging_id),
            u64::try_from(content_range.start).map_err(|_| UploadApiError::Internal)?,
            Box::pin(reader),
        )
        .await
        .map_err(|_| UploadApiError::Internal)?;
    let expected_length = content_range
        .end
        .checked_sub(content_range.start)
        .and_then(|difference| difference.checked_add(1))
        .and_then(|length| u64::try_from(length).ok())
        .ok_or(UploadApiError::BadRequest("Invalid byte range"))?;
    if written != expected_length {
        return Err(UploadApiError::BadRequest(
            "Request body length does not match Content-Range",
        ));
    }

    let session = strife_db::record_chunk(
        &state.pool,
        session_id,
        content_range.start,
        content_range.end,
    )
    .await
    .map_err(|error| match error {
        RecordChunkError::NotFound => UploadApiError::NotFound,
        RecordChunkError::NotActive => UploadApiError::Gone,
        RecordChunkError::Overlap => UploadApiError::RangeConflict,
        RecordChunkError::Database(_) => UploadApiError::Internal,
    })?;
    Ok(Json(UploadProgressResponse {
        received_bytes: session.received_bytes,
        expected_bytes: session.expected_byte_size,
        complete: session
            .expected_byte_size
            .is_some_and(|expected| session.received_bytes == expected),
    }))
}

fn parse_content_range(value: &str) -> Option<ContentRange> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse::<i64>().ok()?;
    let end = end.parse::<i64>().ok()?;
    if start < 0 || end < start {
        return None;
    }
    let total = if total == "*" {
        None
    } else {
        let parsed = total.parse::<i64>().ok()?;
        if parsed <= end {
            return None;
        }
        Some(parsed)
    };
    Some(ContentRange { start, end, total })
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505"))
}

#[cfg(test)]
mod tests {
    use super::{ContentRange, parse_content_range};

    #[test]
    fn parses_valid_content_ranges() {
        assert_eq!(
            parse_content_range("bytes 0-4/10"),
            Some(ContentRange {
                start: 0,
                end: 4,
                total: Some(10)
            })
        );
        assert_eq!(
            parse_content_range("bytes 5-9/*"),
            Some(ContentRange {
                start: 5,
                end: 9,
                total: None
            })
        );
    }

    #[test]
    fn rejects_invalid_content_ranges() {
        for value in ["0-4/10", "bytes 5-4/10", "bytes 0-10/10", "bytes a-b/10"] {
            assert_eq!(parse_content_range(value), None);
        }
    }
}
