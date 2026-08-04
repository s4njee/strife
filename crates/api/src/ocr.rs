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

use crate::error::ApiError;

const ROUTE: &str = "/api/ocr";
const DEFAULT_TREE_LIMIT: u32 = 100;

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

#[derive(Debug, Deserialize)]
struct OcrTreeQuery {
    parent_id: Option<Uuid>,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
struct OcrTreeNodeResponse {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    kind: String,
    status: Option<String>,
    source: Option<String>,
    page_count: Option<i32>,
    mean_confidence: Option<f32>,
    char_count: Option<i32>,
    updated_at: chrono::DateTime<chrono::Utc>,
    total_files: i64,
    pending: i64,
    running: i64,
    completed: i64,
    failed: i64,
    skipped: i64,
    unsupported: i64,
}

#[derive(Clone, Debug, Serialize)]
struct OcrTreeResponse {
    items: Vec<OcrTreeNodeResponse>,
    next_offset: Option<u32>,
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
        .route("/api/ocr/tree", get(tree))
        .route("/api/ocr/preflight", get(preflight))
        .route("/api/ocr/events", get(events))
        .with_state(pool)
}

/// Lists one lazy level of the OCR document tree.
///
/// Folder rows summarize every active OCR-related descendant, while file rows
/// describe only direct children. This keeps a 40,000-file archive browsable
/// without transferring the whole hierarchy on each request.
#[allow(clippy::too_many_lines)]
async fn tree(
    State(pool): State<PgPool>,
    Query(query): Query<OcrTreeQuery>,
) -> Result<Json<OcrTreeResponse>, ApiError> {
    let parent_id = query.parent_id.unwrap_or(strife_db::ROOT_NODE_ID);
    let limit = query.limit.unwrap_or(DEFAULT_TREE_LIMIT).clamp(1, 200);
    let offset = query.offset.unwrap_or_default();
    let mut rows = sqlx::query!(
        r#"
        WITH RECURSIVE candidates AS (
            -- Only a file OCR has touched can contribute to a rollup. Deriving
            -- the set from these two indexed reads keeps the query
            -- proportional to the OCR corpus rather than to the library: the
            -- previous version sequentially scanned every row in `nodes` on
            -- every expansion, and then discarded all but one folder's
            -- children.
            SELECT node_id AS id FROM document_text
            UNION
            SELECT target_node_id FROM jobs
            WHERE job_type = 'ocr' AND state IN ('pending', 'leased')
        ),
        file_status AS (
            SELECT
                n.id,
                n.parent_id,
                n.name,
                COALESCE(dt.updated_at, n.updated_at) AS updated_at,
                CASE
                    WHEN active.state = 'leased' THEN 'running'
                    WHEN active.state = 'pending' THEN 'pending'
                    WHEN dt.status = 'completed' AND dt.source = 'embedded' THEN 'skipped'
                    ELSE dt.status::text
                END AS status,
                dt.source::text AS source,
                dt.page_count,
                dt.mean_confidence,
                dt.char_count
            FROM candidates
            JOIN nodes n ON n.id = candidates.id
            LEFT JOIN document_text dt ON dt.node_id = n.id
            LEFT JOIN LATERAL (
                SELECT state
                FROM jobs
                WHERE target_node_id = n.id
                  AND job_type = 'ocr'
                  AND state IN ('pending', 'leased')
                ORDER BY CASE WHEN state = 'leased' THEN 0 ELSE 1 END, created_at, id
                LIMIT 1
            ) active ON TRUE
            WHERE n.kind = 'file'
              AND n.lifecycle_state = 'active'
        ),
        folder_direct AS (
            -- Aggregate before recursing. Carrying one subtotal per folder up
            -- the tree moves thousands of rows where carrying one row per file
            -- moved hundreds of thousands.
            SELECT
                parent_id AS folder_id,
                count(*) AS total_files,
                count(*) FILTER (WHERE status = 'pending') AS pending,
                count(*) FILTER (WHERE status = 'running') AS running,
                count(*) FILTER (WHERE status = 'completed') AS completed,
                count(*) FILTER (WHERE status = 'failed') AS failed,
                count(*) FILTER (WHERE status = 'skipped') AS skipped,
                count(*) FILTER (WHERE status = 'unsupported') AS unsupported
            FROM file_status
            WHERE parent_id IS NOT NULL
            GROUP BY parent_id
        ),
        ancestry AS (
            SELECT folder_id, total_files, pending, running, completed,
                   failed, skipped, unsupported
            FROM folder_direct
            UNION ALL
            -- The parent lookup is a correlated primary-key subquery rather
            -- than a join. A join lets the planner hash the whole of `nodes`
            -- once per recursion level, which on a thirteen-level tree meant
            -- thirteen full-table hashes and dominated the query.
            SELECT
                (SELECT parent.parent_id FROM nodes parent WHERE parent.id = a.folder_id),
                a.total_files, a.pending, a.running, a.completed,
                a.failed, a.skipped, a.unsupported
            FROM ancestry a
            WHERE (SELECT parent.parent_id FROM nodes parent WHERE parent.id = a.folder_id)
                  IS NOT NULL
        ),
        folder_counts AS (
            SELECT
                folder_id,
                sum(total_files)::bigint AS total_files,
                sum(pending)::bigint AS pending,
                sum(running)::bigint AS running,
                sum(completed)::bigint AS completed,
                sum(failed)::bigint AS failed,
                sum(skipped)::bigint AS skipped,
                sum(unsupported)::bigint AS unsupported
            FROM ancestry
            GROUP BY folder_id
        ),
        tree_nodes AS (
            SELECT
                child.id,
                child.parent_id,
                child.name,
                'folder'::text AS kind,
                NULL::text AS status,
                NULL::text AS source,
                NULL::integer AS page_count,
                NULL::real AS mean_confidence,
                NULL::integer AS char_count,
                child.updated_at,
                counts.total_files,
                counts.pending,
                counts.running,
                counts.completed,
                counts.failed,
                counts.skipped,
                counts.unsupported
            FROM nodes child
            JOIN folder_counts counts ON counts.folder_id = child.id
            WHERE child.parent_id = $1
              AND child.kind = 'folder'
              AND child.lifecycle_state = 'active'
            UNION ALL
            SELECT
                file.id,
                file.parent_id,
                file.name,
                'file'::text,
                file.status,
                file.source,
                file.page_count,
                file.mean_confidence,
                file.char_count,
                file.updated_at,
                1::bigint,
                CASE WHEN file.status = 'pending' THEN 1 ELSE 0 END,
                CASE WHEN file.status = 'running' THEN 1 ELSE 0 END,
                CASE WHEN file.status = 'completed' THEN 1 ELSE 0 END,
                CASE WHEN file.status = 'failed' THEN 1 ELSE 0 END,
                CASE WHEN file.status = 'skipped' THEN 1 ELSE 0 END,
                CASE WHEN file.status = 'unsupported' THEN 1 ELSE 0 END
            FROM file_status file
            WHERE file.parent_id = $1
        )
        SELECT
            id AS "id!",
            parent_id,
            name AS "name!",
            kind AS "kind!",
            status,
            source,
            page_count,
            mean_confidence,
            char_count,
            updated_at AS "updated_at!",
            total_files AS "total_files!",
            pending AS "pending!",
            running AS "running!",
            completed AS "completed!",
            failed AS "failed!",
            skipped AS "skipped!",
            unsupported AS "unsupported!"
        FROM tree_nodes
        ORDER BY (kind = 'folder') DESC, lower(name), id
        LIMIT $2 OFFSET $3
        "#,
        parent_id,
        i64::from(limit.saturating_add(1)),
        i64::from(offset),
    )
    .fetch_all(&pool)
    .await
    .map_err(|error| ApiError::internal(error, ROUTE, "document tree"))?;
    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }
    Ok(Json(OcrTreeResponse {
        items: rows
            .into_iter()
            .map(|row| OcrTreeNodeResponse {
                id: row.id,
                parent_id: row.parent_id,
                name: row.name,
                kind: row.kind,
                status: row.status,
                source: row.source,
                page_count: row.page_count,
                mean_confidence: row.mean_confidence,
                char_count: row.char_count,
                updated_at: row.updated_at,
                total_files: row.total_files,
                pending: row.pending,
                running: row.running,
                completed: row.completed,
                failed: row.failed,
                skipped: row.skipped,
                unsupported: row.unsupported,
            })
            .collect(),
        next_offset: has_more.then(|| offset.saturating_add(limit)),
    }))
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
