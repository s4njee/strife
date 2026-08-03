use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use strife_db::{
    BackfillCampaignEventRecord, BackfillCampaignRecord, BackfillKind, BackfillState,
    JobResourceClass, NewBackfillCampaign,
};
use uuid::Uuid;

use crate::error::ApiError;

const ROUTE: &str = "/api/backfills";

#[derive(Deserialize)]
struct CreateCampaignRequest {
    kind: String,
    #[serde(default)]
    candidate_definition: serde_json::Value,
    #[serde(default = "default_batch_size")]
    batch_size: i32,
    #[serde(default = "default_max_queued")]
    max_queued: i32,
    #[serde(default = "default_max_running")]
    max_running: i32,
    resource_class: String,
    #[serde(default = "default_foreground_fairness")]
    foreground_fairness: i32,
}

#[derive(Deserialize)]
struct PrepareCampaignRequest {
    candidate_count: i64,
    snapshot_before: DateTime<Utc>,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct CampaignActionRequest {
    action: String,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct CanaryResultRequest {
    stage: i64,
    processed: i64,
    failed: i64,
    duration_seconds: f64,
    p50_seconds: f64,
    p95_seconds: f64,
    peak_cpu_percent: f64,
    peak_memory_bytes: i64,
    peak_temperature_c: f64,
    peak_io_wait_percent: f64,
    database_growth_bytes: i64,
    approved: bool,
}

#[derive(Deserialize)]
struct EventQuery {
    campaign_id: Option<Uuid>,
}

#[derive(Serialize)]
struct CampaignResponse {
    id: Uuid,
    kind: String,
    state: String,
    candidate_definition: serde_json::Value,
    snapshot_before: Option<DateTime<Utc>>,
    cursor_created_at: Option<DateTime<Utc>>,
    cursor_node_id: Option<Uuid>,
    batch_size: i32,
    max_queued: i32,
    max_running: i32,
    resource_class: String,
    foreground_fairness: i32,
    candidate_count: i64,
    enqueued_count: i64,
    completed_count: i64,
    failed_count: i64,
    skipped_count: i64,
    created_by_version: String,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    paused_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct CampaignEventResponse {
    id: i64,
    campaign_id: Uuid,
    old_state: Option<String>,
    new_state: Option<String>,
    event_type: String,
    reason: Option<String>,
    details: serde_json::Value,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct CampaignMetricsResponse {
    pending: i64,
    running: i64,
    remaining: i64,
    throughput_per_hour: Option<f64>,
    estimated_completion_at: Option<DateTime<Utc>>,
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/backfills", get(list).post(create))
        .route("/api/backfills/events", get(events))
        .route("/api/backfills/{id}", get(get_one))
        .route("/api/backfills/{id}/metrics", get(metrics))
        .route(
            "/api/backfills/{id}/canary-results",
            get(canary_results).post(record_canary_result),
        )
        .route("/api/backfills/{id}/prepare", post(prepare))
        .route("/api/backfills/{id}/actions", post(action))
        .with_state(pool)
}

async fn list(State(pool): State<PgPool>) -> Result<Json<Vec<CampaignResponse>>, ApiError> {
    let campaigns = strife_db::list_backfill_campaigns(&pool)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, "campaign list"))?;
    Ok(Json(campaigns.into_iter().map(Into::into).collect()))
}

async fn get_one(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Json<CampaignResponse>, ApiError> {
    strife_db::get_backfill_campaign(&pool, id)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, id))?
        .map(CampaignResponse::from)
        .map(Json)
        .ok_or(ApiError::NotFound("Backfill campaign was not found"))
}

async fn create(
    State(pool): State<PgPool>,
    Json(request): Json<CreateCampaignRequest>,
) -> Result<(StatusCode, Json<CampaignResponse>), ApiError> {
    if request.batch_size <= 0
        || request.max_queued <= 0
        || request.max_running <= 0
        || request.foreground_fairness <= 0
        || !request.candidate_definition.is_object()
        || request
            .candidate_definition
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .is_none()
    {
        return Err(ApiError::Unprocessable(
            "Backfill campaign configuration is invalid".into(),
        ));
    }
    let campaign = strife_db::create_backfill_campaign(
        &pool,
        &NewBackfillCampaign {
            kind: parse_kind(&request.kind)
                .ok_or_else(|| ApiError::Unprocessable("Unknown backfill kind".into()))?,
            candidate_definition: request.candidate_definition,
            batch_size: request.batch_size,
            max_queued: request.max_queued,
            max_running: request.max_running,
            resource_class: parse_resource_class(&request.resource_class)
                .ok_or_else(|| ApiError::Unprocessable("Unknown resource class".into()))?,
            foreground_fairness: request.foreground_fairness,
            created_by_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    )
    .await
    .map_err(|error| ApiError::internal(error, ROUTE, "campaign create"))?;
    Ok((StatusCode::CREATED, Json(campaign.into())))
}

async fn prepare(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    Json(request): Json<PrepareCampaignRequest>,
) -> Result<Json<CampaignResponse>, ApiError> {
    if request.candidate_count < 0 {
        return Err(ApiError::Unprocessable(
            "Candidate count cannot be negative".into(),
        ));
    }
    strife_db::prepare_backfill_campaign(
        &pool,
        id,
        request.candidate_count,
        request.snapshot_before,
        request.reason.as_deref(),
    )
    .await
    .map_err(|error| ApiError::internal(error, ROUTE, id))?
    .map(CampaignResponse::from)
    .map(Json)
    .ok_or_else(|| ApiError::Conflict("Backfill campaign cannot be prepared".into()))
}

async fn action(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    Json(request): Json<CampaignActionRequest>,
) -> Result<Json<CampaignResponse>, ApiError> {
    let target = match request.action.as_str() {
        "resume" => BackfillState::Running,
        "pause" => BackfillState::Paused,
        "drain" => BackfillState::Draining,
        "complete" => BackfillState::Completed,
        "cancel" => BackfillState::Cancelled,
        "fail" => BackfillState::Failed,
        _ => return Err(ApiError::Unprocessable("Unknown campaign action".into())),
    };
    strife_db::transition_backfill_campaign(&pool, id, target, request.reason.as_deref())
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, id))?
        .map(CampaignResponse::from)
        .map(Json)
        .ok_or_else(|| ApiError::Conflict("Backfill campaign cannot change state".into()))
}

async fn metrics(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Json<CampaignMetricsResponse>, ApiError> {
    let campaign = strife_db::get_backfill_campaign(&pool, id)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, id))?
        .ok_or(ApiError::NotFound("Backfill campaign was not found"))?;
    let measurements = strife_db::get_backfill_campaign_metrics(&pool, id)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, id))?
        .ok_or(ApiError::NotFound("Backfill campaign was not found"))?;
    let remaining = campaign
        .candidate_count
        .saturating_sub(campaign.completed_count)
        .saturating_sub(campaign.failed_count)
        .saturating_sub(campaign.skipped_count)
        .max(0);
    let throughput_per_hour = if measurements.completed_last_hour >= 2
        && measurements.observed_seconds >= 1.0
    {
        #[allow(clippy::cast_precision_loss)]
        Some((measurements.completed_last_hour as f64 * 3_600.0) / measurements.observed_seconds)
    } else {
        None
    };
    let estimated_completion_at = throughput_per_hour.and_then(|throughput| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let seconds = ((remaining as f64 / throughput) * 3_600.0).round() as i64;
        Utc::now().checked_add_signed(chrono::Duration::seconds(seconds))
    });
    Ok(Json(CampaignMetricsResponse {
        pending: measurements.pending,
        running: measurements.running,
        remaining,
        throughput_per_hour,
        estimated_completion_at,
    }))
}

async fn canary_results(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CampaignEventResponse>>, ApiError> {
    let results = strife_db::list_backfill_canary_results(&pool, id)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, id))?;
    Ok(Json(results.into_iter().map(Into::into).collect()))
}

async fn record_canary_result(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    Json(request): Json<CanaryResultRequest>,
) -> Result<(StatusCode, Json<CampaignEventResponse>), ApiError> {
    let measurements = [
        request.duration_seconds,
        request.p50_seconds,
        request.p95_seconds,
        request.peak_cpu_percent,
        request.peak_temperature_c,
        request.peak_io_wait_percent,
    ];
    if !matches!(request.stage, 100 | 1_000 | 10_000)
        || request.processed < 0
        || request.processed > request.stage
        || request.failed < 0
        || request.failed > request.processed
        || request.peak_memory_bytes < 0
        || request.database_growth_bytes < 0
        || measurements
            .into_iter()
            .any(|value| !value.is_finite() || value < 0.0)
    {
        return Err(ApiError::Unprocessable(
            "Canary measurements are invalid".into(),
        ));
    }
    #[allow(clippy::cast_precision_loss)]
    let throughput_per_hour = if request.duration_seconds > 0.0 {
        request.processed as f64 * 3_600.0 / request.duration_seconds
    } else {
        0.0
    };
    let details = serde_json::json!({
        "stage": request.stage,
        "processed": request.processed,
        "failed": request.failed,
        "duration_seconds": request.duration_seconds,
        "throughput_per_hour": throughput_per_hour,
        "p50_seconds": request.p50_seconds,
        "p95_seconds": request.p95_seconds,
        "peak_cpu_percent": request.peak_cpu_percent,
        "peak_memory_bytes": request.peak_memory_bytes,
        "peak_temperature_c": request.peak_temperature_c,
        "peak_io_wait_percent": request.peak_io_wait_percent,
        "database_growth_bytes": request.database_growth_bytes,
        "approved": request.approved,
    });
    let result = strife_db::record_backfill_canary_result(&pool, id, &details)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, id))?
        .ok_or_else(|| {
            ApiError::Conflict("Pause the campaign before recording canary results".into())
        })?;
    Ok((StatusCode::CREATED, Json(result.into())))
}

async fn events(
    State(pool): State<PgPool>,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default();
    let stream = futures_util::stream::unfold(
        (pool, query.campaign_id, cursor, Vec::<Event>::new()),
        |(pool, campaign_id, mut cursor, mut pending)| async move {
            loop {
                if let Some(event) = pending.pop() {
                    return Some((
                        Ok::<_, Infallible>(event),
                        (pool, campaign_id, cursor, pending),
                    ));
                }
                match strife_db::list_backfill_campaign_events_after(
                    &pool,
                    campaign_id,
                    cursor,
                    100,
                )
                .await
                {
                    Ok(records) if records.is_empty() => {
                        tokio::time::sleep(Duration::from_millis(750)).await;
                    }
                    Ok(records) => {
                        let mut next = Vec::with_capacity(records.len());
                        for record in records {
                            cursor = record.id;
                            next.push(
                                Event::default()
                                    .event("campaign")
                                    .id(record.id.to_string())
                                    .json_data(CampaignEventResponse::from(record))
                                    .unwrap_or_else(|_| Event::default().event("stream-error")),
                            );
                        }
                        pending = next.into_iter().rev().collect();
                    }
                    Err(error) => {
                        tracing::error!(%error, "backfill event stream query failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
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

impl From<BackfillCampaignRecord> for CampaignResponse {
    fn from(value: BackfillCampaignRecord) -> Self {
        Self {
            id: value.id,
            kind: kind_name(value.kind).into(),
            state: state_name(value.state).into(),
            candidate_definition: value.candidate_definition,
            snapshot_before: value.snapshot_before,
            cursor_created_at: value.cursor_created_at,
            cursor_node_id: value.cursor_node_id,
            batch_size: value.batch_size,
            max_queued: value.max_queued,
            max_running: value.max_running,
            resource_class: resource_class_name(value.resource_class).into(),
            foreground_fairness: value.foreground_fairness,
            candidate_count: value.candidate_count,
            enqueued_count: value.enqueued_count,
            completed_count: value.completed_count,
            failed_count: value.failed_count,
            skipped_count: value.skipped_count,
            created_by_version: value.created_by_version,
            last_error: value.last_error,
            created_at: value.created_at,
            updated_at: value.updated_at,
            started_at: value.started_at,
            paused_at: value.paused_at,
            completed_at: value.completed_at,
        }
    }
}

impl From<BackfillCampaignEventRecord> for CampaignEventResponse {
    fn from(value: BackfillCampaignEventRecord) -> Self {
        Self {
            id: value.id,
            campaign_id: value.campaign_id,
            old_state: value.old_state.map(|state| state_name(state).into()),
            new_state: value.new_state.map(|state| state_name(state).into()),
            event_type: value.event_type,
            reason: value.reason,
            details: value.details,
            created_at: value.created_at,
        }
    }
}

const fn default_batch_size() -> i32 {
    100
}
const fn default_max_queued() -> i32 {
    500
}
const fn default_max_running() -> i32 {
    1
}
const fn default_foreground_fairness() -> i32 {
    20
}

fn parse_kind(value: &str) -> Option<BackfillKind> {
    match value {
        "email" => Some(BackfillKind::Email),
        "ocr" => Some(BackfillKind::Ocr),
        "attachment_text" => Some(BackfillKind::AttachmentText),
        "attachment_ocr" => Some(BackfillKind::AttachmentOcr),
        _ => None,
    }
}
fn parse_resource_class(value: &str) -> Option<JobResourceClass> {
    match value {
        "light" => Some(JobResourceClass::Light),
        "extractor" => Some(JobResourceClass::Extractor),
        "preview" => Some(JobResourceClass::Preview),
        "heavy_cpu" => Some(JobResourceClass::HeavyCpu),
        "heavy_io" => Some(JobResourceClass::HeavyIo),
        _ => None,
    }
}
const fn kind_name(value: BackfillKind) -> &'static str {
    match value {
        BackfillKind::Email => "email",
        BackfillKind::Ocr => "ocr",
        BackfillKind::AttachmentText => "attachment_text",
        BackfillKind::AttachmentOcr => "attachment_ocr",
    }
}
const fn state_name(value: BackfillState) -> &'static str {
    match value {
        BackfillState::Draft => "draft",
        BackfillState::Paused => "paused",
        BackfillState::Running => "running",
        BackfillState::Draining => "draining",
        BackfillState::Completed => "completed",
        BackfillState::Cancelled => "cancelled",
        BackfillState::Failed => "failed",
    }
}
const fn resource_class_name(value: JobResourceClass) -> &'static str {
    match value {
        JobResourceClass::Light => "light",
        JobResourceClass::Extractor => "extractor",
        JobResourceClass::Preview => "preview",
        JobResourceClass::HeavyCpu => "heavy_cpu",
        JobResourceClass::HeavyIo => "heavy_io",
    }
}
