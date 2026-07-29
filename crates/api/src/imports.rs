use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

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
use strife_db::{ImportEntryRecord, ImportEntryState, ImportSourceRecord};
use strife_importer::{PostgresDiscoverySink, ScanOptions, import_entry, scan_directory};
use strife_storage::{DiskGuard, StorageBackend};
use uuid::Uuid;

#[derive(Clone)]
struct ImportState {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    storage_root: PathBuf,
    watch_root: PathBuf,
    disk_guard: DiskGuard,
}

/// Builds the fixed watched-folder management API.
///
/// # Panics
///
/// Panics when `disk_guard_percent` is outside the inclusive range 1..=100.
pub fn router(
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    storage_root: PathBuf,
    watch_root: PathBuf,
    disk_guard_percent: u8,
) -> Router {
    Router::new()
        .route("/api/import-sources", get(list_sources))
        .route("/api/import-sources/{id}", patch(update_source))
        .route("/api/import-sources/{id}/scan", post(scan_source))
        .route("/api/import-sources/{id}/entries", get(list_entries))
        .route(
            "/api/import-sources/{id}/entries/{entry_id}/retry",
            post(retry_entry),
        )
        .with_state(ImportState {
            pool,
            storage,
            storage_root,
            watch_root,
            disk_guard: DiskGuard::new(disk_guard_percent)
                .expect("disk guard percentage must be between 1 and 100"),
        })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportCountsResponse {
    pub discovered: i64,
    pub stable: i64,
    pub importing: i64,
    pub imported: i64,
    pub failed: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportSourceResponse {
    pub id: Uuid,
    pub watch_path: String,
    pub destination_folder_id: Uuid,
    pub enabled: bool,
    pub last_scan_at: Option<DateTime<Utc>>,
    pub counts: ImportCountsResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportEntryResponse {
    pub id: Uuid,
    pub source_path: String,
    pub source_size: i64,
    pub source_modified_at: DateTime<Utc>,
    pub state: String,
    pub resulting_node_id: Option<Uuid>,
    pub error_message: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScanResponse {
    pub discovered: usize,
    pub imported: usize,
    pub failed: usize,
    pub skipped_hidden: usize,
    pub skipped_special: usize,
}

#[derive(Debug, Deserialize)]
struct UpdateSourceRequest {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct EntryQuery {
    state: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

#[derive(Debug)]
enum ImportApiError {
    BadRequest(&'static str),
    NotFound,
    Disabled,
    Internal,
}

impl IntoResponse for ImportApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Import source was not found",
            ),
            Self::Disabled => (
                StatusCode::CONFLICT,
                "source_disabled",
                "Import source is disabled",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Import operation failed",
            ),
        };
        (status, Json(ErrorBody { code, message })).into_response()
    }
}

async fn list_sources(
    State(state): State<ImportState>,
) -> Result<Json<Vec<ImportSourceResponse>>, ImportApiError> {
    let sources = strife_db::list_import_source_statuses(&state.pool)
        .await
        .map_err(|_| ImportApiError::Internal)?;
    Ok(Json(
        sources
            .into_iter()
            .map(|source| ImportSourceResponse {
                id: source.id,
                watch_path: source.watch_path,
                destination_folder_id: source.destination_folder_id,
                enabled: source.enabled,
                last_scan_at: source.last_scan_at,
                counts: ImportCountsResponse {
                    discovered: source.discovered_count,
                    stable: source.stable_count,
                    importing: source.importing_count,
                    imported: source.imported_count,
                    failed: source.failed_count,
                },
            })
            .collect(),
    ))
}

async fn update_source(
    State(state): State<ImportState>,
    Path(source_id): Path<Uuid>,
    Json(request): Json<UpdateSourceRequest>,
) -> Result<Json<ImportSourceResponse>, ImportApiError> {
    strife_db::set_import_source_enabled(&state.pool, source_id, request.enabled)
        .await
        .map_err(|_| ImportApiError::Internal)?
        .ok_or(ImportApiError::NotFound)?;
    let source = strife_db::list_import_source_statuses(&state.pool)
        .await
        .map_err(|_| ImportApiError::Internal)?
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or(ImportApiError::NotFound)?;
    Ok(Json(ImportSourceResponse {
        id: source.id,
        watch_path: source.watch_path,
        destination_folder_id: source.destination_folder_id,
        enabled: source.enabled,
        last_scan_at: source.last_scan_at,
        counts: ImportCountsResponse {
            discovered: source.discovered_count,
            stable: source.stable_count,
            importing: source.importing_count,
            imported: source.imported_count,
            failed: source.failed_count,
        },
    }))
}

async fn scan_source(
    State(state): State<ImportState>,
    Path(source_id): Path<Uuid>,
) -> Result<Json<ScanResponse>, ImportApiError> {
    let source = load_enabled_source(&state.pool, source_id).await?;
    validate_watch_path(&state.watch_root, &state.storage_root).await?;
    let sink = PostgresDiscoverySink::new(&state.pool);
    let scan = scan_directory(&state.watch_root, source.id, ScanOptions::default(), &sink)
        .await
        .map_err(|_| ImportApiError::BadRequest("Watch path could not be scanned"))?;
    let entries = strife_db::list_pending_entries(&state.pool, source.id)
        .await
        .map_err(|_| ImportApiError::Internal)?;
    let mut imported = 0;
    let mut failed = 0;
    for entry in entries {
        match import_entry(
            &state.pool,
            state.storage.as_ref(),
            &state.watch_root,
            source.destination_folder_id,
            &entry,
            state.disk_guard,
        )
        .await
        {
            Ok(Some(_)) => imported += 1,
            Ok(None) => {}
            Err(_) => failed += 1,
        }
    }
    strife_db::mark_import_source_scanned(&state.pool, source.id)
        .await
        .map_err(|_| ImportApiError::Internal)?;
    Ok(Json(ScanResponse {
        discovered: scan.files_discovered,
        imported,
        failed,
        skipped_hidden: scan.hidden_entries_skipped,
        skipped_special: scan.special_entries_skipped,
    }))
}

async fn list_entries(
    State(state): State<ImportState>,
    Path(source_id): Path<Uuid>,
    Query(query): Query<EntryQuery>,
) -> Result<Json<Vec<ImportEntryResponse>>, ImportApiError> {
    let filter = query.state.as_deref().map(parse_entry_state).transpose()?;
    let entries = strife_db::list_import_entries(&state.pool, source_id, filter)
        .await
        .map_err(|_| ImportApiError::Internal)?;
    Ok(Json(entries.into_iter().map(Into::into).collect()))
}

async fn retry_entry(
    State(state): State<ImportState>,
    Path((source_id, entry_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ImportEntryResponse>, ImportApiError> {
    let entry = strife_db::retry_import_entry(&state.pool, source_id, entry_id)
        .await
        .map_err(|_| ImportApiError::Internal)?
        .ok_or(ImportApiError::NotFound)?;
    Ok(Json(entry.into()))
}

async fn load_enabled_source(
    pool: &PgPool,
    source_id: Uuid,
) -> Result<ImportSourceRecord, ImportApiError> {
    let source = strife_db::get_import_source(pool, source_id)
        .await
        .map_err(|_| ImportApiError::Internal)?
        .ok_or(ImportApiError::NotFound)?;
    if source.enabled {
        Ok(source)
    } else {
        Err(ImportApiError::Disabled)
    }
}

async fn validate_watch_path(
    watch_root: &FsPath,
    storage_root: &FsPath,
) -> Result<(), ImportApiError> {
    let watch = tokio::fs::canonicalize(watch_root)
        .await
        .map_err(|_| ImportApiError::BadRequest("Watch path does not exist or is unreadable"))?;
    let storage = tokio::fs::canonicalize(storage_root)
        .await
        .map_err(|_| ImportApiError::Internal)?;
    if watch.starts_with(&storage) || storage.starts_with(&watch) {
        return Err(ImportApiError::BadRequest(
            "Watch path cannot overlap managed storage",
        ));
    }
    let _reader = tokio::fs::read_dir(&watch)
        .await
        .map_err(|_| ImportApiError::BadRequest("Watch path does not exist or is unreadable"))?;
    Ok(())
}

fn parse_entry_state(value: &str) -> Result<ImportEntryState, ImportApiError> {
    match value {
        "discovered" => Ok(ImportEntryState::Discovered),
        "stable" => Ok(ImportEntryState::Stable),
        "importing" => Ok(ImportEntryState::Importing),
        "imported" => Ok(ImportEntryState::Imported),
        "failed" => Ok(ImportEntryState::Failed),
        _ => Err(ImportApiError::BadRequest("Unknown import entry state")),
    }
}

impl From<ImportEntryRecord> for ImportEntryResponse {
    fn from(entry: ImportEntryRecord) -> Self {
        Self {
            id: entry.id,
            source_path: entry.source_path,
            source_size: entry.source_size,
            source_modified_at: entry.source_modified_at,
            state: match entry.state {
                ImportEntryState::Discovered => "discovered",
                ImportEntryState::Stable => "stable",
                ImportEntryState::Importing => "importing",
                ImportEntryState::Imported => "imported",
                ImportEntryState::Failed => "failed",
            }
            .to_owned(),
            resulting_node_id: entry.resulting_node_id,
            error_message: entry.error_message,
            updated_at: entry.updated_at,
        }
    }
}
