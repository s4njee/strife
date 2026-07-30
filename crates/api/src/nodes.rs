//! Trash, restore, favorites, and other node-lifecycle HTTP endpoints.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use strife_db::{NodeKind, NodeRecord, TrashEntryRecord, TrashMutationError};
use strife_domain::FolderError;
use uuid::Uuid;

#[derive(Clone)]
struct NodesState {
    pool: PgPool,
}

/// Builds node lifecycle routes (trash, restore, permanent deletion, favorites).
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/trash", get(list_trash))
        .route("/api/nodes/{id}/trash", post(trash_node))
        .route("/api/nodes/trash", post(trash_nodes_batch))
        .route("/api/nodes/{id}/restore", post(restore_node))
        .route("/api/nodes/{id}/permanent", delete(permanent_delete))
        .with_state(NodesState { pool })
}

#[derive(Debug, Deserialize)]
struct TrashBatchRequest {
    node_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKindResponse {
    Folder,
    File,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NodeResponse {
    pub id: Uuid,
    pub name: String,
    pub kind: NodeKindResponse,
    pub parent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TrashItemResponse {
    pub id: Uuid,
    pub node_id: Uuid,
    pub name: String,
    pub kind: NodeKindResponse,
    pub original_parent_id: Option<Uuid>,
    pub trashed_at: DateTime<Utc>,
    pub scheduled_purge_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TrashListResponse {
    pub items: Vec<TrashItemResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TrashBatchResponse {
    pub items: Vec<NodeResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PermanentDeleteResponse {
    pub node_id: Uuid,
    pub job_id: Option<Uuid>,
    pub status: String,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

#[derive(Debug)]
enum ApiError {
    BadRequest(&'static str),
    NotFound,
    NotTrashed,
    CannotTrashRoot,
    NameConflict,
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, "bad_request", message.to_owned())
            }
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Node was not found".to_owned(),
            ),
            Self::NotTrashed => (
                StatusCode::CONFLICT,
                "not_trashed",
                "Node is not in the trash".to_owned(),
            ),
            Self::CannotTrashRoot => (
                StatusCode::BAD_REQUEST,
                "cannot_trash_root",
                "The root folder cannot be moved to trash".to_owned(),
            ),
            Self::NameConflict => (
                StatusCode::CONFLICT,
                "name_conflict",
                "An active sibling already has this name".to_owned(),
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The node operation could not be completed".to_owned(),
            ),
        };
        (status, Json(ErrorBody { code, message })).into_response()
    }
}

impl From<TrashMutationError> for ApiError {
    fn from(error: TrashMutationError) -> Self {
        match error {
            TrashMutationError::Rule(FolderError::NotFound) => Self::NotFound,
            TrashMutationError::Rule(FolderError::NameConflict) => Self::NameConflict,
            TrashMutationError::Rule(FolderError::InvalidName | FolderError::CycleDetected) => {
                Self::Internal
            }
            TrashMutationError::CannotTrashRoot => Self::CannotTrashRoot,
            TrashMutationError::NotTrashed => Self::NotTrashed,
            TrashMutationError::Database(_) => Self::Internal,
        }
    }
}

async fn trash_node(
    State(state): State<NodesState>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<NodeResponse>, ApiError> {
    let node = strife_db::trash_node(&state.pool, node_id).await?;
    Ok(Json(node.into()))
}

async fn trash_nodes_batch(
    State(state): State<NodesState>,
    Json(request): Json<TrashBatchRequest>,
) -> Result<Json<TrashBatchResponse>, ApiError> {
    if request.node_ids.is_empty() {
        return Err(ApiError::BadRequest("Select at least one item"));
    }
    let nodes = strife_db::trash_nodes(&state.pool, &request.node_ids).await?;
    Ok(Json(TrashBatchResponse {
        items: nodes.into_iter().map(NodeResponse::from).collect(),
    }))
}

async fn restore_node(
    State(state): State<NodesState>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<NodeResponse>, ApiError> {
    let node = strife_db::restore_node(&state.pool, node_id).await?;
    Ok(Json(node.into()))
}

async fn list_trash(
    State(state): State<NodesState>,
) -> Result<Json<TrashListResponse>, ApiError> {
    let items = strife_db::list_trash(&state.pool)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(TrashListResponse {
        items: items.into_iter().map(TrashItemResponse::from).collect(),
    }))
}

async fn permanent_delete(
    State(state): State<NodesState>,
    Path(node_id): Path<Uuid>,
) -> Result<(StatusCode, Json<PermanentDeleteResponse>), ApiError> {
    let job = strife_db::request_permanent_deletion(&state.pool, node_id).await?;
    let (status, body_status, job_id) = if let Some(job) = job {
        (StatusCode::ACCEPTED, "queued", Some(job.id))
    } else {
        // Already deleted, or an identical job is already pending/leased.
        let existing = strife_db::get_node_by_id(&state.pool, node_id)
            .await
            .map_err(|_| ApiError::Internal)?;
        if existing.is_none() {
            (StatusCode::OK, "already_deleted", None)
        } else {
            (StatusCode::ACCEPTED, "already_queued", None)
        }
    };
    Ok((
        status,
        Json(PermanentDeleteResponse {
            node_id,
            job_id,
            status: body_status.to_owned(),
        }),
    ))
}

impl From<NodeRecord> for NodeResponse {
    fn from(node: NodeRecord) -> Self {
        Self {
            id: node.id,
            name: node.name,
            kind: match node.kind {
                NodeKind::Folder => NodeKindResponse::Folder,
                NodeKind::File => NodeKindResponse::File,
            },
            parent_id: node.parent_id,
            created_at: node.created_at,
            updated_at: node.updated_at,
        }
    }
}

impl From<TrashEntryRecord> for TrashItemResponse {
    fn from(entry: TrashEntryRecord) -> Self {
        Self {
            id: entry.id,
            node_id: entry.node_id,
            name: entry.name,
            kind: match entry.kind {
                NodeKind::Folder => NodeKindResponse::Folder,
                NodeKind::File => NodeKindResponse::File,
            },
            original_parent_id: entry.original_parent_id,
            trashed_at: entry.trashed_at,
            scheduled_purge_at: entry.scheduled_purge_at,
            created_at: entry.created_at,
            updated_at: entry.updated_at,
        }
    }
}

