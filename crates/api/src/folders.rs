use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use strife_db::{FolderMutationError, NodeKind, NodeRecord};
use uuid::Uuid;

const DEFAULT_PAGE_SIZE: u32 = 50;
const MAX_PAGE_SIZE: u32 = 100;

#[derive(Clone)]
struct FolderState {
    pool: PgPool,
}

/// Builds the folder-management API router.
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/folders", post(create_folder))
        .route("/api/folders/{id}", patch(update_folder))
        .route("/api/folders/{id}/children", get(list_children))
        .with_state(FolderState { pool })
}

#[derive(Debug, Deserialize)]
struct ListChildrenQuery {
    cursor: Option<Uuid>,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CreateFolderRequest {
    parent_id: Uuid,
    name: String,
}

#[derive(Debug, Deserialize)]
struct UpdateFolderRequest {
    name: Option<String>,
    parent_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKindResponse {
    Folder,
    File,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FolderResponse {
    pub id: Uuid,
    pub name: String,
    pub kind: NodeKindResponse,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChildrenResponse {
    pub items: Vec<FolderResponse>,
    pub next_cursor: Option<Uuid>,
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
    NameConflict,
    CycleDetected,
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
                "Folder or destination was not found".to_owned(),
            ),
            Self::NameConflict => (
                StatusCode::CONFLICT,
                "name_conflict",
                "An active sibling already has this name".to_owned(),
            ),
            Self::CycleDetected => (
                StatusCode::BAD_REQUEST,
                "cycle_detected",
                "Moving this folder would create a cycle".to_owned(),
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The folder operation could not be completed".to_owned(),
            ),
        };

        (status, Json(ErrorBody { code, message })).into_response()
    }
}

impl From<FolderMutationError> for ApiError {
    fn from(error: FolderMutationError) -> Self {
        match error {
            FolderMutationError::NotFound => Self::NotFound,
            FolderMutationError::NameConflict => Self::NameConflict,
            FolderMutationError::CycleDetected => Self::CycleDetected,
            FolderMutationError::Database(_) => Self::Internal,
        }
    }
}

async fn list_children(
    State(state): State<FolderState>,
    Path(folder_id): Path<Uuid>,
    Query(query): Query<ListChildrenQuery>,
) -> Result<Json<ChildrenResponse>, ApiError> {
    let folder = strife_db::get_node_by_id(&state.pool, folder_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .filter(|node| node.kind == NodeKind::Folder)
        .ok_or(ApiError::NotFound)?;
    if folder.lifecycle_state != strife_db::LifecycleState::Active {
        return Err(ApiError::NotFound);
    }

    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let mut nodes = strife_db::list_children_page(
        &state.pool,
        folder_id,
        query.cursor,
        limit.saturating_add(1),
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    let has_more = nodes.len() > limit as usize;
    if has_more {
        nodes.pop();
    }
    let next_cursor = has_more.then(|| nodes.last().map(|node| node.id)).flatten();

    Ok(Json(ChildrenResponse {
        items: nodes.into_iter().map(FolderResponse::from).collect(),
        next_cursor,
    }))
}

async fn create_folder(
    State(state): State<FolderState>,
    Json(request): Json<CreateFolderRequest>,
) -> Result<(StatusCode, Json<FolderResponse>), ApiError> {
    let name = validate_name(&request.name)?;
    let folder = strife_db::create_folder(&state.pool, request.parent_id, name).await?;
    Ok((StatusCode::CREATED, Json(folder.into())))
}

async fn update_folder(
    State(state): State<FolderState>,
    Path(folder_id): Path<Uuid>,
    Json(request): Json<UpdateFolderRequest>,
) -> Result<Json<FolderResponse>, ApiError> {
    if request.name.is_none() && request.parent_id.is_none() {
        return Err(ApiError::BadRequest("Provide a name or parent_id"));
    }
    let name = request.name.as_deref().map(validate_name).transpose()?;
    let folder = strife_db::update_folder(&state.pool, folder_id, name, request.parent_id).await?;
    Ok(Json(folder.into()))
}

fn validate_name(name: &str) -> Result<&str, ApiError> {
    if name.is_empty() {
        Err(ApiError::BadRequest("Folder name cannot be empty"))
    } else {
        Ok(name)
    }
}

impl From<NodeRecord> for FolderResponse {
    fn from(node: NodeRecord) -> Self {
        Self {
            id: node.id,
            name: node.name,
            kind: match node.kind {
                NodeKind::Folder => NodeKindResponse::Folder,
                NodeKind::File => NodeKindResponse::File,
            },
            created_at: node.created_at,
            updated_at: node.updated_at,
        }
    }
}
