use std::{
    convert::Infallible,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, Sse, sse::Event, sse::KeepAlive},
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use strife_db::{ImportEntryRecord, ImportEntryState, ImportSourceRecord};
use strife_storage::StorageBackend;
use uuid::Uuid;

use crate::log_internal;

#[derive(Clone)]
struct ImportState {
    pool: PgPool,
    storage_root: PathBuf,
    watch_root: PathBuf,
}

/// Builds the fixed watched-folder management API.
///
pub fn router(
    pool: PgPool,
    _storage: Arc<dyn StorageBackend>,
    storage_root: PathBuf,
    watch_root: PathBuf,
    _disk_guard_percent: u8,
) -> Router {
    Router::new()
        .route("/api/import-sources", get(list_sources))
        .route("/api/import-sources/{id}", patch(update_source))
        .route("/api/import-sources/{id}/scan", post(scan_source))
        .route("/api/import-sources/{id}/entries", get(list_entries))
        .route("/api/import-sources/{id}/events", get(stream_import_events))
        .route(
            "/api/import-sources/{id}/entries/{entry_id}/retry",
            post(retry_entry),
        )
        .with_state(ImportState {
            pool,
            storage_root,
            watch_root,
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
    pub job_id: Uuid,
    pub status: String,
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
        .map_err(|error| log_internal(error, ImportApiError::Internal))?;
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
        .map_err(|error| log_internal(error, ImportApiError::Internal))?
        .ok_or(ImportApiError::NotFound)?;
    let source = strife_db::list_import_source_statuses(&state.pool)
        .await
        .map_err(|error| log_internal(error, ImportApiError::Internal))?
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
) -> Result<(StatusCode, Json<ScanResponse>), ImportApiError> {
    load_enabled_source(&state.pool, source_id).await?;
    validate_watch_path(&state.watch_root, &state.storage_root).await?;
    let job = strife_db::enqueue_import_scan(&state.pool, source_id)
        .await
        .map_err(|error| log_internal(error, ImportApiError::Internal))?
        .ok_or(ImportApiError::Disabled)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ScanResponse {
            job_id: job.id,
            status: format!("{:?}", job.state).to_lowercase(),
        }),
    ))
}

async fn list_entries(
    State(state): State<ImportState>,
    Path(source_id): Path<Uuid>,
    Query(query): Query<EntryQuery>,
) -> Result<Json<Vec<ImportEntryResponse>>, ImportApiError> {
    let filter = query.state.as_deref().map(parse_entry_state).transpose()?;
    let entries = strife_db::list_import_entries(&state.pool, source_id, filter)
        .await
        .map_err(|error| log_internal(error, ImportApiError::Internal))?;
    Ok(Json(entries.into_iter().map(Into::into).collect()))
}

#[allow(clippy::too_many_lines)]
async fn stream_import_events(
    State(state): State<ImportState>,
    Path(source_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ImportApiError> {
    strife_db::get_import_source(&state.pool, source_id)
        .await
        .map_err(|error| log_internal(error, ImportApiError::Internal))?
        .ok_or(ImportApiError::NotFound)?;

    let resume_cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_event_cursor);
    let (cursor_time, cursor_id) = match resume_cursor {
        Some(cursor) => cursor,
        None => (
            sqlx::query_scalar::<_, DateTime<Utc>>("SELECT now()")
                .fetch_one(&state.pool)
                .await
                .map_err(|error| log_internal(error, ImportApiError::Internal))?,
            Uuid::nil(),
        ),
    };
    let stream = futures_util::stream::unfold(
        (
            state.pool,
            source_id,
            cursor_time,
            cursor_id,
            Vec::<Event>::new(),
            None::<ImportSourceResponse>,
        ),
        |(pool, source_id, mut cursor_time, mut cursor_id, mut pending, mut previous_source)| async move {
            loop {
                if let Some(event) = pending.pop() {
                    return Some((
                        Ok::<_, Infallible>(event),
                        (
                            pool,
                            source_id,
                            cursor_time,
                            cursor_id,
                            pending,
                            previous_source,
                        ),
                    ));
                }

                let entries = match strife_db::list_import_entry_events_after(
                    &pool,
                    source_id,
                    cursor_time,
                    cursor_id,
                    100,
                )
                .await
                {
                    Ok(entries) => entries,
                    Err(error) => {
                        tracing::error!(?error, %source_id, "import event stream query failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                let source = match load_source_response(&pool, source_id).await {
                    Ok(source) => source,
                    Err(error) => {
                        tracing::error!(?error, %source_id, "import status stream query failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                let mut events = Vec::with_capacity(entries.len() + 1);
                for entry in entries {
                    cursor_time = entry.updated_at;
                    cursor_id = entry.id;
                    let response = ImportEntryResponse::from(entry);
                    events.push(
                        Event::default()
                            .event("entry")
                            .id(format!(
                                "{}:{}",
                                response.updated_at.timestamp_micros(),
                                response.id
                            ))
                            .json_data(response)
                            .unwrap_or_else(|_| Event::default().event("stream-error")),
                    );
                }
                if previous_source.as_ref() != Some(&source) {
                    events.push(
                        Event::default()
                            .event("status")
                            .json_data(&source)
                            .unwrap_or_else(|_| Event::default().event("stream-error")),
                    );
                    previous_source = Some(source);
                }

                if events.is_empty() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                } else {
                    pending = events.into_iter().rev().collect();
                }
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

async fn load_source_response(
    pool: &PgPool,
    source_id: Uuid,
) -> Result<ImportSourceResponse, sqlx::Error> {
    let source = strife_db::list_import_source_statuses(pool)
        .await?
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or(sqlx::Error::RowNotFound)?;
    Ok(ImportSourceResponse {
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
}

fn parse_event_cursor(value: &str) -> Option<(DateTime<Utc>, Uuid)> {
    let (micros, id) = value.split_once(':')?;
    Some((
        DateTime::from_timestamp_micros(micros.parse().ok()?)?,
        id.parse().ok()?,
    ))
}

async fn retry_entry(
    State(state): State<ImportState>,
    Path((source_id, entry_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ImportEntryResponse>, ImportApiError> {
    let entry = strife_db::retry_import_entry(&state.pool, source_id, entry_id)
        .await
        .map_err(|error| log_internal(error, ImportApiError::Internal))?
        .ok_or(ImportApiError::NotFound)?;
    Ok(Json(entry.into()))
}

async fn load_enabled_source(
    pool: &PgPool,
    source_id: Uuid,
) -> Result<ImportSourceRecord, ImportApiError> {
    let source = strife_db::get_import_source(pool, source_id)
        .await
        .map_err(|error| log_internal(error, ImportApiError::Internal))?
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
        .map_err(|error| log_internal(error, ImportApiError::Internal))?;
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
