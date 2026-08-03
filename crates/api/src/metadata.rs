use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::{Query, State},
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

use crate::internal_error;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataCountsResponse {
    pub pending: i64,
    pub running: i64,
    pub completed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub cancelled: i64,
    pub remaining: i64,
    pub total: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataStatusResponse {
    pub counts: MetadataCountsResponse,
    pub completed_per_hour: i64,
    pub eta_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataEventResponse {
    pub id: i64,
    pub job_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
    pub name: String,
    pub state: String,
    pub attempt: i32,
    pub extractor_name: Option<String>,
    pub duration_ms: Option<i64>,
    pub error: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct RecentQuery {
    limit: Option<i64>,
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/metadata/status", get(status))
        .route("/api/metadata/recent", get(recent))
        .route("/api/metadata/events", get(events))
        .with_state(pool)
}

async fn status(
    State(pool): State<PgPool>,
) -> Result<Json<MetadataStatusResponse>, axum::http::StatusCode> {
    Ok(Json(load_status(&pool).await.map_err(internal_error)?))
}

async fn recent(
    State(pool): State<PgPool>,
    Query(query): Query<RecentQuery>,
) -> Result<Json<Vec<MetadataEventResponse>>, axum::http::StatusCode> {
    let records = strife_db::list_recent_metadata_events(&pool, query.limit.unwrap_or(50))
        .await
        .map_err(internal_error)?;
    Ok(Json(records.into_iter().map(Into::into).collect()))
}

async fn load_status(pool: &PgPool) -> Result<MetadataStatusResponse, sqlx::Error> {
    let counts = strife_db::get_metadata_status_counts(pool).await?;
    let eta_seconds = (counts.completed_per_hour > 0)
        .then(|| counts.remaining.saturating_mul(3600) / counts.completed_per_hour);
    Ok(MetadataStatusResponse {
        counts: MetadataCountsResponse {
            pending: counts.pending,
            running: counts.running,
            completed: counts.completed,
            failed: counts.failed,
            skipped: counts.skipped,
            cancelled: counts.cancelled,
            remaining: counts.remaining,
            total: counts.total,
        },
        completed_per_hour: counts.completed_per_hour,
        eta_seconds,
    })
}

impl From<strife_db::MetadataEventRecord> for MetadataEventResponse {
    fn from(record: strife_db::MetadataEventRecord) -> Self {
        Self {
            id: record.id,
            job_id: record.job_id,
            node_id: record.node_id,
            name: record.node_name,
            state: record.state,
            attempt: record.attempt,
            extractor_name: record.extractor_name,
            duration_ms: record.duration_ms,
            error: record.error_message,
            created_at: record.created_at,
        }
    }
}

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
        None => sqlx::query_scalar::<_, i64>("SELECT COALESCE(max(id), 0) FROM metadata_events")
            .fetch_one(&pool)
            .await
            .unwrap_or_default(),
    };
    let stream = futures_util::stream::unfold(
        (
            pool,
            cursor,
            Vec::<Event>::new(),
            None::<MetadataStatusResponse>,
            tokio::time::Instant::now(),
        ),
        |(pool, mut cursor, mut pending, mut previous_status, mut next_status_at)| async move {
            loop {
                if let Some(event) = pending.pop() {
                    return Some((
                        Ok::<_, Infallible>(event),
                        (pool, cursor, pending, previous_status, next_status_at),
                    ));
                }
                let records = match strife_db::list_metadata_events_after(&pool, cursor, 100).await
                {
                    Ok(records) => records,
                    Err(error) => {
                        tracing::error!(%error, "metadata event stream query failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let mut next = Vec::with_capacity(records.len() + 1);
                for record in records {
                    cursor = record.id;
                    let response = MetadataEventResponse::from(record);
                    next.push(
                        Event::default()
                            .event("entry")
                            .id(response.id.to_string())
                            .json_data(response)
                            .unwrap_or_else(|_| Event::default().event("stream-error")),
                    );
                }
                let now = tokio::time::Instant::now();
                if now >= next_status_at {
                    let current_status = match load_status(&pool).await {
                        Ok(status) => status,
                        Err(error) => {
                            tracing::error!(%error, "metadata status stream query failed");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    next_status_at = now + STATUS_REFRESH_INTERVAL;
                    if previous_status.as_ref() != Some(&current_status) {
                        next.push(
                            Event::default()
                                .event("status")
                                .json_data(&current_status)
                                .unwrap_or_else(|_| Event::default().event("stream-error")),
                        );
                        previous_status = Some(current_status);
                    }
                }
                if next.is_empty() {
                    tokio::time::sleep(EVENT_POLL_INTERVAL).await;
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
