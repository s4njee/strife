use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Serialize)]
struct JobStatus {
    id: Uuid,
    status: String,
    error: Option<String>,
}

#[derive(Serialize)]
struct JobCountResponse {
    count: i64,
}

#[derive(Deserialize)]
struct JobListQuery {
    state: Option<String>,
    count: Option<bool>,
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/jobs/{id}", get(status))
        .route("/api/jobs", get(list_or_count))
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

async fn list_or_count(
    State(pool): State<PgPool>,
    Query(query): Query<JobListQuery>,
) -> Result<Json<JobCountResponse>, StatusCode> {
    let states = query
        .state
        .as_deref()
        .unwrap_or("pending,leased")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    let count: i64 = sqlx::query_scalar(
        r"
        SELECT COUNT(*)
        FROM jobs
        WHERE state::text = ANY($1)
        ",
    )
    .bind(&states)
    .fetch_one(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _ = query.count;
    Ok(Json(JobCountResponse { count }))
}
