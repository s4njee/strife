use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use strife_db::{CreateUploadSession, LifecycleState, NodeKind};
use strife_domain::FolderRules;
use strife_storage::StorageBackend;
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

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505"))
}
