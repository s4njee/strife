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
use strife_db::{
    ChildrenKindFilter, ChildrenSort, FolderMoveConflict, FolderMoveConflictReason,
    FolderMutationError, NodeKind, NodeRecord, SortOrder,
};
use strife_domain::{FolderError, FolderRules};
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
        .route("/api/folders/move", patch(move_folders))
        .route("/api/folders/{id}", patch(update_folder))
        .route("/api/folders/{id}/children", get(list_children))
        .route("/api/folders/{id}/ancestors", get(list_ancestors))
        .with_state(FolderState { pool })
}

#[derive(Debug, Deserialize)]
struct ListChildrenQuery {
    cursor: Option<Uuid>,
    limit: Option<u32>,
    /// Sort column: `name`, `kind`, `size`, `updated_at` / `modified`, `created_at` / `created`.
    sort: Option<String>,
    /// Sort direction: `asc` or `desc`.
    order: Option<String>,
    /// Kind filters: `folder`, `image`, `document`, `video`, `audio` (repeatable).
    #[serde(default)]
    kind: Vec<String>,
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

#[derive(Debug, Deserialize)]
struct MoveFoldersRequest {
    folder_ids: Vec<Uuid>,
    parent_id: Uuid,
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
    #[serde(default)]
    pub is_favorite: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChildrenResponse {
    pub items: Vec<FolderResponse>,
    pub next_cursor: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AncestorResponse {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MoveFoldersResponse {
    pub items: Vec<FolderResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveConflictReasonResponse {
    NameConflict,
    CycleDetected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MoveConflictResponse {
    pub id: Uuid,
    pub name: String,
    pub reason: MoveConflictReasonResponse,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct MoveConflictBody {
    code: &'static str,
    message: &'static str,
    conflicts: Vec<MoveConflictResponse>,
}

#[derive(Debug)]
enum ApiError {
    BadRequest(&'static str),
    NotFound,
    NameConflict,
    CycleDetected,
    MoveConflict(Vec<MoveConflictResponse>),
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::MoveConflict(conflicts) = self {
            return (
                StatusCode::CONFLICT,
                Json(MoveConflictBody {
                    code: "move_conflict",
                    message: "One or more folders cannot be moved",
                    conflicts,
                }),
            )
                .into_response();
        }

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
            Self::MoveConflict(_) => unreachable!(),
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
            FolderMutationError::Rule(FolderError::NotFound) => Self::NotFound,
            FolderMutationError::Rule(FolderError::NameConflict) => Self::NameConflict,
            FolderMutationError::Rule(FolderError::CycleDetected) => Self::CycleDetected,
            FolderMutationError::Rule(FolderError::InvalidName) => {
                Self::BadRequest("Folder name cannot be empty")
            }
            FolderMutationError::MoveConflict(conflicts) => Self::MoveConflict(
                conflicts
                    .into_iter()
                    .map(MoveConflictResponse::from)
                    .collect(),
            ),
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
    let sort = parse_sort(query.sort.as_deref())?;
    let order = parse_order(query.order.as_deref())?;
    let kinds = parse_kinds(&query.kind)?;
    let mut nodes = strife_db::list_children_page_sorted(
        &state.pool,
        folder_id,
        query.cursor,
        limit.saturating_add(1),
        sort,
        order,
        &kinds,
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    let has_more = nodes.len() > limit as usize;
    if has_more {
        nodes.pop();
    }
    let next_cursor = has_more.then(|| nodes.last().map(|node| node.id)).flatten();
    let node_ids: Vec<Uuid> = nodes.iter().map(|node| node.id).collect();
    let favorite_ids = strife_db::favorite_ids_among(&state.pool, &node_ids)
        .await
        .map_err(|_| ApiError::Internal)?;
    let favorite_set: std::collections::HashSet<Uuid> = favorite_ids.into_iter().collect();

    Ok(Json(ChildrenResponse {
        items: nodes
            .into_iter()
            .map(|node| {
                let mut response = FolderResponse::from(node);
                response.is_favorite = favorite_set.contains(&response.id);
                response
            })
            .collect(),
        next_cursor,
    }))
}

fn parse_sort(value: Option<&str>) -> Result<ChildrenSort, ApiError> {
    match value.unwrap_or("name") {
        "name" => Ok(ChildrenSort::Name),
        "kind" | "type" => Ok(ChildrenSort::Kind),
        "size" => Ok(ChildrenSort::Size),
        "updated_at" | "modified" | "date" => Ok(ChildrenSort::UpdatedAt),
        "created_at" | "created" => Ok(ChildrenSort::CreatedAt),
        _ => Err(ApiError::BadRequest("Unknown sort column")),
    }
}

fn parse_order(value: Option<&str>) -> Result<SortOrder, ApiError> {
    match value.unwrap_or("asc") {
        "asc" => Ok(SortOrder::Asc),
        "desc" => Ok(SortOrder::Desc),
        _ => Err(ApiError::BadRequest("order must be asc or desc")),
    }
}

fn parse_kinds(values: &[String]) -> Result<Vec<ChildrenKindFilter>, ApiError> {
    let mut kinds = Vec::new();
    for value in values {
        for part in value.split(',') {
            let part = part.trim();
            if part.is_empty() || part == "all" {
                continue;
            }
            kinds.push(match part {
                "folder" | "folders" => ChildrenKindFilter::Folder,
                "image" | "images" => ChildrenKindFilter::Image,
                "document" | "documents" => ChildrenKindFilter::Document,
                "video" | "videos" => ChildrenKindFilter::Video,
                "audio" => ChildrenKindFilter::Audio,
                _ => return Err(ApiError::BadRequest("Unknown kind filter")),
            });
        }
    }
    kinds.sort_by_key(|kind| format!("{kind:?}"));
    kinds.dedup();
    Ok(kinds)
}

async fn list_ancestors(
    State(state): State<FolderState>,
    Path(folder_id): Path<Uuid>,
) -> Result<Json<Vec<AncestorResponse>>, ApiError> {
    let ancestors = strife_db::list_ancestors(&state.pool, folder_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    if ancestors.is_empty() {
        return Err(ApiError::NotFound);
    }

    Ok(Json(
        ancestors
            .into_iter()
            .map(|node| AncestorResponse {
                id: node.id,
                name: node.name,
            })
            .collect(),
    ))
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

async fn move_folders(
    State(state): State<FolderState>,
    Json(request): Json<MoveFoldersRequest>,
) -> Result<Json<MoveFoldersResponse>, ApiError> {
    if request.folder_ids.is_empty() {
        return Err(ApiError::BadRequest("Select at least one folder"));
    }

    let folders =
        strife_db::move_folders(&state.pool, &request.folder_ids, request.parent_id).await?;
    Ok(Json(MoveFoldersResponse {
        items: folders.into_iter().map(FolderResponse::from).collect(),
    }))
}

fn validate_name(name: &str) -> Result<&str, ApiError> {
    FolderRules::validate_name(name)
        .map_err(|_| ApiError::BadRequest("Folder name cannot be empty"))
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
            is_favorite: false,
        }
    }
}

impl From<FolderMoveConflict> for MoveConflictResponse {
    fn from(conflict: FolderMoveConflict) -> Self {
        Self {
            id: conflict.id,
            name: conflict.name,
            reason: match conflict.reason {
                FolderMoveConflictReason::NameConflict => MoveConflictReasonResponse::NameConflict,
                FolderMoveConflictReason::CycleDetected => {
                    MoveConflictReasonResponse::CycleDetected
                }
            },
        }
    }
}
