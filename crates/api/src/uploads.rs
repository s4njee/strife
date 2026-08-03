use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{patch, post},
};
use chrono::{DateTime, Duration, Utc};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use strife_db::{
    CreateUploadSession, FinalizeUploadError, LifecycleState, NodeKind, RecordChunkError,
    UploadSessionState,
};
use strife_domain::FolderRules;
use strife_storage::{DiskGuard, StorageBackend, StorageKey};
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;
use uuid::Uuid;

use crate::error::ApiError;
use crate::folders::FolderResponse;

#[derive(Clone)]
struct UploadState {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    session_ttl: Duration,
    disk_guard: DiskGuard,
}

/// Builds the resumable-upload API router.
///
/// # Panics
///
/// Panics when `disk_guard_percent` is outside the inclusive range 1..=100.
pub fn router(
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    session_ttl: Duration,
    disk_guard_percent: u8,
) -> Router {
    Router::new()
        .route("/api/uploads", post(create_upload).get(list_uploads))
        .route(
            "/api/uploads/{id}",
            patch(upload_chunk).get(get_upload).delete(cancel_upload),
        )
        .route("/api/uploads/{id}/finalize", post(finalize_upload))
        .with_state(UploadState {
            pool,
            storage,
            session_ttl,
            disk_guard: DiskGuard::new(disk_guard_percent)
                .expect("disk guard percentage must be between 1 and 100"),
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReceivedRangeResponse {
    pub start: i64,
    pub end: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UploadSessionResponse {
    pub session_id: Uuid,
    pub state: &'static str,
    pub display_name: String,
    pub received_bytes: i64,
    pub expected_bytes: Option<i64>,
    pub received_ranges: Vec<ReceivedRangeResponse>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ListUploadsQuery {
    folder_id: Uuid,
}

/// Messages preserved verbatim from the per-module error type this replaced.
const NOT_FOUND: &str = "Target folder was not found";
const NAME_CONFLICT: &str = "An active item or upload already has this name";
const INTERNAL: &str = "The upload session could not be created";
const ROUTE: &str = "/api/uploads";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContentRange {
    start: i64,
    end: i64,
    total: Option<i64>,
}

async fn create_upload(
    State(state): State<UploadState>,
    Json(request): Json<CreateUploadRequest>,
) -> Result<(StatusCode, Json<CreateUploadResponse>), ApiError> {
    FolderRules::validate_name(&request.name)
        .map_err(|_| ApiError::BadRequest("Upload name cannot be empty".into()))?;
    if request.size.is_some_and(|size| size < 0) {
        return Err(ApiError::BadRequest(
            "Upload size cannot be negative".into(),
        ));
    }

    let folder = strife_db::get_node_by_id(&state.pool, request.folder_id)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?
        .filter(|node| {
            node.kind == NodeKind::Folder && node.lifecycle_state == LifecycleState::Active
        })
        .ok_or(ApiError::NotFound(NOT_FOUND))?;
    if strife_db::active_child_name_exists(&state.pool, folder.id, &request.name)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?
    {
        return Err(ApiError::NameConflict(NAME_CONFLICT));
    }

    let usage = state
        .storage
        .disk_usage()
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    let incoming_bytes = request
        .size
        .and_then(|size| u64::try_from(size).ok())
        .unwrap_or_default();
    state
        .disk_guard
        .check(usage, incoming_bytes)
        .map_err(|error| ApiError::DiskFull(error.usage_percent))?;

    let staging_key = Uuid::new_v4();
    state
        .storage
        .put_stream(
            StorageKey::staging(staging_key),
            Box::pin(tokio::io::empty()),
        )
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    let session_result = strife_db::create_session(
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
    .await;
    let session = match session_result {
        Ok(session) => session,
        Err(error) => {
            let _ = state.storage.delete(StorageKey::staging(staging_key)).await;
            return Err(if is_unique_violation(&error) {
                ApiError::NameConflict(NAME_CONFLICT)
            } else {
                ApiError::Internal(INTERNAL)
            });
        }
    };

    Ok((
        StatusCode::CREATED,
        Json(CreateUploadResponse {
            session_id: session.id,
            staging_key,
        }),
    ))
}

async fn get_upload(
    State(state): State<UploadState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<UploadSessionResponse>, ApiError> {
    let progress = strife_db::get_session_progress(&state.pool, session_id)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?
        .ok_or(ApiError::NotFound(NOT_FOUND))?;
    Ok(Json(progress_response(progress)))
}

async fn list_uploads(
    State(state): State<UploadState>,
    Query(query): Query<ListUploadsQuery>,
) -> Result<Json<Vec<UploadSessionResponse>>, ApiError> {
    let sessions = strife_db::list_active_upload_sessions(&state.pool, query.folder_id)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    let mut responses = Vec::with_capacity(sessions.len());
    for session in sessions {
        let progress = strife_db::get_session_progress(&state.pool, session.id)
            .await
            .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?
            .ok_or(ApiError::NotFound(NOT_FOUND))?;
        responses.push(progress_response(progress));
    }
    Ok(Json(responses))
}

fn progress_response(progress: strife_db::UploadSessionProgress) -> UploadSessionResponse {
    UploadSessionResponse {
        session_id: progress.session.id,
        state: upload_state_name(progress.session.state),
        display_name: progress.session.display_name,
        received_bytes: progress.session.received_bytes,
        expected_bytes: progress.session.expected_byte_size,
        received_ranges: progress
            .received_ranges
            .into_iter()
            .map(|range| ReceivedRangeResponse {
                start: range.start_byte,
                end: range.end_byte,
            })
            .collect(),
        created_at: progress.session.created_at,
        expires_at: progress.session.expires_at,
    }
}

const fn upload_state_name(state: UploadSessionState) -> &'static str {
    match state {
        UploadSessionState::Active => "active",
        UploadSessionState::Finalizing => "finalizing",
        UploadSessionState::Completed => "completed",
        UploadSessionState::Cancelled => "cancelled",
        UploadSessionState::Expired => "expired",
    }
}

async fn finalize_upload(
    State(state): State<UploadState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<FolderResponse>, ApiError> {
    let progress = strife_db::get_session_progress(&state.pool, session_id)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?
        .ok_or(ApiError::NotFound(NOT_FOUND))?;
    if progress.session.state == UploadSessionState::Completed {
        let node_id = progress
            .session
            .completed_node_id
            .ok_or(ApiError::Internal(INTERNAL))?;
        let node = strife_db::get_node_by_id(&state.pool, node_id)
            .await
            .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?
            .ok_or(ApiError::NotFound(NOT_FOUND))?;
        return Ok(Json(node.into()));
    }
    if progress.session.state != UploadSessionState::Active {
        return Err(ApiError::UploadInactive);
    }
    if progress
        .session
        .expected_byte_size
        .is_some_and(|expected| expected != progress.session.received_bytes)
    {
        return Err(ApiError::UploadIncomplete);
    }

    let staging_id = Uuid::parse_str(&progress.session.staging_key)
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    let staging_key = StorageKey::staging(staging_id);
    if !state
        .storage
        .exists(staging_key)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?
    {
        return Err(ApiError::NotFound(NOT_FOUND));
    }
    let checksum = checksum_reader(
        state
            .storage
            .get_stream(staging_key)
            .await
            .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?,
    )
    .await?;
    let mime_type = state
        .storage
        .detect_mime(staging_key)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    let original_id = Uuid::new_v4();
    let original_key = StorageKey::original(original_id);
    state
        .storage
        .move_object(staging_key, original_key)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    let finalized = strife_db::finalize_upload(
        &state.pool,
        session_id,
        original_id,
        progress.session.received_bytes,
        &mime_type,
        &checksum,
    )
    .await;
    match finalized {
        Ok(node) => Ok(Json(node.into())),
        Err(error) => {
            let _ = state.storage.move_object(original_key, staging_key).await;
            if matches!(error, FinalizeUploadError::NotActive)
                && strife_db::get_upload_session(&state.pool, session_id)
                    .await
                    .is_ok_and(|session| {
                        session.is_some_and(|session| {
                            matches!(
                                session.state,
                                UploadSessionState::Cancelled | UploadSessionState::Expired
                            )
                        })
                    })
            {
                let _ = state.storage.delete(staging_key).await;
            }
            Err(match error {
                FinalizeUploadError::NotFound => ApiError::NotFound(NOT_FOUND),
                FinalizeUploadError::NotActive => ApiError::UploadInactive,
                FinalizeUploadError::Incomplete => ApiError::UploadIncomplete,
                FinalizeUploadError::NameConflict => ApiError::NameConflict(NAME_CONFLICT),
                FinalizeUploadError::Database(error) => {
                    ApiError::internal_with(error, ROUTE, "upload", INTERNAL)
                }
            })
        }
    }
}

async fn cancel_upload(
    State(state): State<UploadState>,
    Path(session_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let session = strife_db::cancel_session(&state.pool, session_id)
        .await
        .map_err(|error| {
            if matches!(error, sqlx::Error::RowNotFound) {
                ApiError::NotFound(NOT_FOUND)
            } else {
                ApiError::Internal(INTERNAL)
            }
        })?;
    if session.state != UploadSessionState::Cancelled {
        return Err(ApiError::UploadInactive);
    }
    let staging_id = Uuid::parse_str(&session.staging_key)
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    state
        .storage
        .delete(StorageKey::staging(staging_id))
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes staging objects for overdue sessions and marks each session expired.
/// A storage failure leaves the session active so the next sweep can retry it.
///
/// # Errors
///
/// Returns an error when expired sessions cannot be listed or a successfully
/// deleted session cannot be transitioned to its terminal state.
pub async fn cleanup_expired_uploads(
    pool: &PgPool,
    storage: &dyn StorageBackend,
) -> anyhow::Result<usize> {
    let sessions = strife_db::list_expired_sessions(pool).await?;
    let mut expired_count = 0;
    for session in sessions {
        let staging_id = Uuid::parse_str(&session.staging_key)?;
        if storage
            .delete(StorageKey::staging(staging_id))
            .await
            .is_err()
        {
            continue;
        }
        let expired = strife_db::expire_session(pool, session.id).await?;
        if expired.state == UploadSessionState::Expired {
            expired_count += 1;
        }
    }
    Ok(expired_count)
}

async fn checksum_reader(mut reader: strife_storage::StorageReader) -> Result<String, ApiError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

async fn upload_chunk(
    State(state): State<UploadState>,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<UploadProgressResponse>, ApiError> {
    let content_range = headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)
        .ok_or(ApiError::BadRequest(
            "A valid Content-Range header is required".into(),
        ))?;
    let progress = strife_db::get_session_progress(&state.pool, session_id)
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?
        .ok_or(ApiError::NotFound(NOT_FOUND))?;
    if progress.session.state != UploadSessionState::Active {
        return Err(ApiError::UploadInactive);
    }
    if content_range.total.is_some() && content_range.total != progress.session.expected_byte_size {
        return Err(ApiError::BadRequest(
            "Content-Range total does not match the upload session".into(),
        ));
    }
    if progress
        .received_ranges
        .iter()
        .any(|range| range.start_byte <= content_range.end && range.end_byte >= content_range.start)
    {
        return Err(ApiError::RangeConflict);
    }

    let staging_id = Uuid::parse_str(&progress.session.staging_key)
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let reader = StreamReader::new(stream);
    let written = state
        .storage
        .write_range(
            StorageKey::staging(staging_id),
            u64::try_from(content_range.start)
                .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?,
            Box::pin(reader),
        )
        .await
        .map_err(|error| ApiError::internal_with(error, ROUTE, "upload", INTERNAL))?;
    let expected_length = content_range
        .end
        .checked_sub(content_range.start)
        .and_then(|difference| difference.checked_add(1))
        .and_then(|length| u64::try_from(length).ok())
        .ok_or(ApiError::BadRequest("Invalid byte range".into()))?;
    if written != expected_length {
        return Err(ApiError::BadRequest(
            "Request body length does not match Content-Range".into(),
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
        RecordChunkError::NotFound => ApiError::NotFound(NOT_FOUND),
        RecordChunkError::NotActive => ApiError::UploadInactive,
        RecordChunkError::Overlap => ApiError::RangeConflict,
        RecordChunkError::Database(error) => {
            ApiError::internal_with(error, ROUTE, "upload", INTERNAL)
        }
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
