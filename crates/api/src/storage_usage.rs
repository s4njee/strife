//! Storage capacity and category breakdown endpoints.

use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde::Serialize;
use sqlx::PgPool;
use strife_storage::{LocalFsBackend, StorageBackend};

use crate::internal_error;

#[derive(Clone)]
struct StorageState {
    pool: PgPool,
    storage: Arc<LocalFsBackend>,
}

/// Builds the storage usage router.
pub fn router(pool: PgPool, storage: Arc<LocalFsBackend>) -> Router {
    Router::new()
        .route("/api/storage/usage", get(usage))
        .with_state(StorageState { pool, storage })
}

#[derive(Clone, Debug, Serialize)]
pub struct StorageUsageResponse {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub originals_bytes: u64,
    pub artifacts_bytes: u64,
    pub trash_bytes: u64,
    pub usage_percent: f64,
}

async fn usage(
    State(state): State<StorageState>,
) -> Result<Json<StorageUsageResponse>, StatusCode> {
    let disk = state.storage.disk_usage().await.map_err(internal_error)?;

    let originals_bytes: i64 = sqlx::query_scalar(
        r"
        SELECT COALESCE(SUM(fo.byte_size), 0)::BIGINT
        FROM file_objects AS fo
        JOIN nodes AS n ON n.id = fo.node_id
        WHERE fo.upload_state = 'finalized'
          AND n.lifecycle_state = 'active'
        ",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(internal_error)?;

    let trash_bytes: i64 = sqlx::query_scalar(
        r"
        SELECT COALESCE(SUM(fo.byte_size), 0)::BIGINT
        FROM file_objects AS fo
        JOIN nodes AS n ON n.id = fo.node_id
        WHERE fo.upload_state = 'finalized'
          AND n.lifecycle_state = 'trashed'
        ",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(internal_error)?;

    let artifacts_bytes: i64 = sqlx::query_scalar(
        r"
        SELECT COALESCE(SUM(byte_size), 0)::BIGINT
        FROM derived_artifacts
        WHERE state = 'ready'
        ",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(internal_error)?;

    #[allow(clippy::cast_precision_loss)]
    let usage_percent = if disk.total_bytes == 0 {
        0.0
    } else {
        (disk.used_bytes as f64 / disk.total_bytes as f64) * 100.0
    };

    Ok(Json(StorageUsageResponse {
        total_bytes: disk.total_bytes,
        used_bytes: disk.used_bytes,
        available_bytes: disk.available_bytes,
        originals_bytes: u64::try_from(originals_bytes.max(0)).unwrap_or(0),
        artifacts_bytes: u64::try_from(artifacts_bytes.max(0)).unwrap_or(0),
        trash_bytes: u64::try_from(trash_bytes.max(0)).unwrap_or(0),
        usage_percent,
    }))
}
