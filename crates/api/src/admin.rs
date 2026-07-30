use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::post,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Builds internal metadata maintenance endpoints.
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/admin/reprocess", post(reprocess))
        .with_state(pool)
}

#[derive(Deserialize)]
struct ReprocessQuery {
    extractor: String,
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
    let current_version = match query.extractor.as_str() {
        "exiftool" | "ffprobe" | "tika" => "adapter-v1",
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let enqueued = strife_db::enqueue_reprocessing(&pool, &query.extractor, current_version)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ReprocessResponse {
        extractor: query.extractor,
        enqueued,
    }))
}
