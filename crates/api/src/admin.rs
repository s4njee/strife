use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::post,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::internal_error;

/// Builds internal metadata maintenance endpoints.
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/admin/reprocess", post(reprocess))
        .with_state(pool)
}

#[derive(Deserialize)]
struct ReprocessQuery {
    extractor: String,
    scope: Option<String>,
    node_id: Option<Uuid>,
}

#[derive(Serialize)]
struct ReprocessResponse {
    extractor: String,
    enqueued: u64,
}

async fn reprocess(
    State(pool): State<PgPool>,
    Query(query): Query<ReprocessQuery>,
) -> Result<Json<ReprocessResponse>, StatusCode> {
    if query.extractor == "ocr" {
        let scope = match query.scope.as_deref() {
            Some("node") => {
                strife_db::OcrReprocessScope::Node(query.node_id.ok_or(StatusCode::BAD_REQUEST)?)
            }
            Some("failed") => strife_db::OcrReprocessScope::Failed,
            Some("version") => {
                let engine = strife_db::get_ocr_engine_state(&pool)
                    .await
                    .map_err(internal_error)?
                    .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
                strife_db::OcrReprocessScope::VersionMismatch(engine.engine_version)
            }
            _ => return Err(StatusCode::BAD_REQUEST),
        };
        let enqueued = strife_db::enqueue_ocr_reprocessing(&pool, &scope, 100)
            .await
            .map_err(internal_error)?;
        return Ok(Json(ReprocessResponse {
            extractor: query.extractor,
            enqueued,
        }));
    }
    let current_version = match query.extractor.as_str() {
        "exiftool" | "ffprobe" | "tika" => "adapter-v1",
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let enqueued = strife_db::enqueue_reprocessing(&pool, &query.extractor, current_version)
        .await
        .map_err(internal_error)?;
    Ok(Json(ReprocessResponse {
        extractor: query.extractor,
        enqueued,
    }))
}
