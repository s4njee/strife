use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{JobType, MIGRATOR, ROOT_NODE_ID, enqueue_job};
use tower::ServiceExt;
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url)
        .await
        .expect("connect to integration-test PostgreSQL");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    Some(pool)
}

async fn json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response");
    serde_json::from_slice(&body).expect("parse response")
}

#[tokio::test]
async fn existing_job_endpoints_return_ocr_jobs_without_shape_changes() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let mut ocr_lock = pool.begin().await.expect("begin OCR test lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(0x4f43_5200_i64)
        .execute(&mut *ocr_lock)
        .await
        .expect("acquire OCR test lock");
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("ocr-api-{node_id}.pdf"))
        .execute(&pool)
        .await
        .expect("create OCR target");
    let job = enqueue_job(&pool, JobType::Ocr, node_id, -10)
        .await
        .expect("enqueue OCR")
        .expect("new OCR job");
    let app = strife_api::jobs::router(pool.clone());

    let status = app
        .clone()
        .oneshot(
            Request::get(format!("/api/jobs/{}", job.id))
                .body(Body::empty())
                .expect("status request"),
        )
        .await
        .expect("status response");
    assert_eq!(status.status(), StatusCode::OK);
    let status = json(status).await;
    assert_eq!(status["id"], job.id.to_string());
    assert_eq!(status["status"], "pending");
    assert!(status["error"].is_null());
    assert_eq!(status.as_object().expect("status object").len(), 3);

    let list = app
        .oneshot(
            Request::get("/api/jobs?state=pending&count=true")
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("list response");
    assert_eq!(list.status(), StatusCode::OK);
    assert!(json(list).await["count"].as_i64().unwrap_or_default() >= 1);

    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("clean up OCR target");
}
