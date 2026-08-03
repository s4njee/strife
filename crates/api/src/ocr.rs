use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ApiError;

const ROUTE: &str = "/api/ocr";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OcrCountsResponse {
    pub pending: i64,
    pub running: i64,
    pub completed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub unsupported: i64,
    pub remaining: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OcrStatusResponse {
    pub counts: OcrCountsResponse,
    pub engine_name: Option<String>,
    pub engine_version: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct OcrEventResponse {
    id: i64,
    node_id: Option<Uuid>,
    name: String,
    state: String,
    page_count: Option<i32>,
    mean_confidence: Option<f32>,
    warning: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OcrPreflightFamilyResponse {
    pub detected_mime: String,
    pub candidates: i64,
    pub total_bytes: i64,
    pub p50_bytes: i64,
    pub p95_bytes: i64,
    pub max_bytes: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OcrPreflightResponse {
    pub snapshot_before: chrono::DateTime<chrono::Utc>,
    pub engine_version: Option<String>,
    pub candidates: i64,
    pub already_completed: i64,
    pub already_skipped: i64,
    pub already_failed: i64,
    pub already_unsupported: i64,
    pub awaiting_metadata: i64,
    pub total_candidate_bytes: i64,
    pub families: Vec<OcrPreflightFamilyResponse>,
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/ocr/status", get(status))
        .route("/api/ocr/preflight", get(preflight))
        .route("/api/ocr/events", get(events))
        .with_state(pool)
}

/// Read-only historical OCR projection.
///
/// This endpoint never enqueues work and never opens a managed original. It
/// exists so an operator can review the candidate set and its size distribution
/// before creating a campaign, and so the campaign's frozen `snapshot_before`
/// and `candidate_count` come from a reviewed report rather than a guess.
async fn preflight(State(pool): State<PgPool>) -> Result<Json<OcrPreflightResponse>, ApiError> {
    let snapshot_before = chrono::Utc::now();
    let engine = strife_db::get_ocr_engine_state(&pool)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, "preflight"))?;
    let supported: Vec<String> = strife_media::supported_ocr_mimes()
        .iter()
        .map(|mime| (*mime).to_owned())
        .collect();
    let report = strife_db::ocr_preflight_report(
        &pool,
        &supported,
        snapshot_before,
        engine.as_ref().map(|value| value.engine_version.as_str()),
    )
    .await
    .map_err(|error| ApiError::internal(error, ROUTE, "preflight"))?;
    Ok(Json(OcrPreflightResponse {
        snapshot_before: report.snapshot_before,
        engine_version: report.engine_version,
        candidates: report.candidates,
        already_completed: report.already_completed,
        already_skipped: report.already_skipped,
        already_failed: report.already_failed,
        already_unsupported: report.already_unsupported,
        awaiting_metadata: report.awaiting_metadata,
        total_candidate_bytes: report.total_candidate_bytes,
        families: report
            .families
            .into_iter()
            .map(|family| OcrPreflightFamilyResponse {
                detected_mime: family.detected_mime,
                candidates: family.candidates,
                total_bytes: family.total_bytes,
                p50_bytes: family.p50_bytes,
                p95_bytes: family.p95_bytes,
                max_bytes: family.max_bytes,
            })
            .collect(),
    }))
}

async fn status(State(pool): State<PgPool>) -> Result<Json<OcrStatusResponse>, ApiError> {
    Ok(Json(load_status(&pool).await.map_err(|error| {
        ApiError::internal(error, ROUTE, "status")
    })?))
}

async fn load_status(pool: &PgPool) -> Result<OcrStatusResponse, sqlx::Error> {
    let counts = strife_db::get_ocr_status_counts(pool).await?;
    let engine = strife_db::get_ocr_engine_state(pool).await?;
    Ok(OcrStatusResponse {
        counts: OcrCountsResponse {
            pending: counts.pending,
            running: counts.running,
            completed: counts.completed,
            failed: counts.failed,
            skipped: counts.skipped,
            unsupported: counts.unsupported,
            remaining: counts.remaining,
        },
        engine_name: engine.as_ref().map(|value| value.engine_name.clone()),
        engine_version: engine.as_ref().map(|value| value.engine_version.clone()),
        language: engine.map(|value| value.language),
    })
}

#[allow(clippy::too_many_lines)]
async fn events(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let header_cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let cursor = match header_cursor {
        Some(cursor) => cursor,
        None => sqlx::query_scalar!("SELECT COALESCE(max(id), 0) AS \"cursor!\" FROM ocr_events")
            .fetch_one(&pool)
            .await
            .unwrap_or_default(),
    };
    let stream = futures_util::stream::unfold(
        (pool, cursor, Vec::<Event>::new(), None::<OcrStatusResponse>),
        |(pool, mut cursor, mut pending, mut previous_status)| async move {
            loop {
                if let Some(event) = pending.pop() {
                    return Some((
                        Ok::<_, Infallible>(event),
                        (pool, cursor, pending, previous_status),
                    ));
                }
                let records = match strife_db::list_ocr_events_after(&pool, cursor, 100).await {
                    Ok(records) => records,
                    Err(error) => {
                        tracing::error!(%error, "OCR event stream query failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let current_status = match load_status(&pool).await {
                    Ok(status) => status,
                    Err(error) => {
                        tracing::error!(%error, "OCR status stream query failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let mut next = Vec::with_capacity(records.len() + 1);
                for record in records {
                    cursor = record.id;
                    let response = OcrEventResponse {
                        id: record.id,
                        node_id: record.node_id,
                        name: record.node_name,
                        state: record.state,
                        page_count: record.page_count,
                        mean_confidence: record.mean_confidence,
                        warning: record.warning,
                        created_at: record.created_at,
                    };
                    next.push(
                        Event::default()
                            .event("entry")
                            .id(record.id.to_string())
                            .json_data(response)
                            .unwrap_or_else(|_| Event::default().event("stream-error")),
                    );
                }
                if previous_status.as_ref() != Some(&current_status) {
                    next.push(
                        Event::default()
                            .event("status")
                            .json_data(&current_status)
                            .unwrap_or_else(|_| Event::default().event("stream-error")),
                    );
                    previous_status = Some(current_status);
                }
                if next.is_empty() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                } else {
                    pending = next.into_iter().rev().collect();
                }
            }
        },
    );
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
