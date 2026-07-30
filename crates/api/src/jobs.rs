use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;
#[derive(Serialize)]
struct JobStatus {
    id: Uuid,
    status: String,
    error: Option<String>,
}
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/jobs/{id}", get(status))
        .with_state(pool)
}
async fn status(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Json<JobStatus>, StatusCode> {
    let job = strife_db::get_job(&pool, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(JobStatus {
        id,
        status: format!("{:?}", job.state).to_lowercase(),
        error: job.last_error,
    }))
}
