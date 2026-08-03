use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Duration;
use futures_util::StreamExt;
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{JobType, MIGRATOR, ROOT_NODE_ID, claim_job, complete_job, enqueue_job};
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

async fn request(app: Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let body = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("response body");
    (
        status,
        serde_json::from_slice(&body).expect("JSON response"),
    )
}

#[tokio::test]
async fn status_recent_and_resumable_sse_follow_metadata_jobs() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let mut lock = pool.begin().await.expect("begin metadata API test lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(0x4d45_5441_i64)
        .execute(&mut *lock)
        .await
        .expect("acquire metadata API test lock");

    let node_id = Uuid::new_v4();
    let name = format!("metadata-console-{node_id}.jpg");
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(&name)
        .execute(&pool)
        .await
        .expect("create metadata target");
    let job = enqueue_job(&pool, JobType::MetadataExtraction, node_id, 10)
        .await
        .expect("enqueue metadata")
        .expect("new metadata job");
    claim_job(
        &pool,
        JobType::MetadataExtraction,
        "metadata-api-test",
        Duration::minutes(1),
    )
    .await
    .expect("claim metadata")
    .expect("leased metadata job");
    complete_job(&pool, job.id)
        .await
        .expect("complete metadata")
        .expect("completed metadata job");

    let app = strife_api::metadata::router(pool.clone());
    let (status_code, status) = request(app.clone(), "/api/metadata/status").await;
    assert_eq!(status_code, StatusCode::OK);
    assert!(status["counts"]["completed"].as_i64().unwrap_or_default() >= 1);
    assert!(status["completed_per_hour"].as_i64().unwrap_or_default() >= 1);

    let (recent_code, recent) = request(app.clone(), "/api/metadata/recent?limit=10").await;
    assert_eq!(recent_code, StatusCode::OK);
    let completion = recent
        .as_array()
        .expect("recent event list")
        .iter()
        .find(|event| event["job_id"] == job.id.to_string() && event["state"] == "completed")
        .expect("completion event");
    assert_eq!(completion["name"], name);
    assert_eq!(completion["attempt"], 1);
    let completion_id = completion["id"].as_i64().expect("completion event id");

    let response = app
        .oneshot(
            Request::get("/api/metadata/events")
                .header("last-event-id", (completion_id - 1).to_string())
                .body(Body::empty())
                .expect("SSE request"),
        )
        .await
        .expect("SSE response");
    let mut stream = response.into_body().into_data_stream();
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .expect("SSE event timeout")
        .expect("SSE event")
        .expect("SSE bytes");
    let event = String::from_utf8_lossy(&chunk);
    assert!(event.contains("event: entry"));
    assert!(event.contains("\"state\":\"completed\""));
    drop(stream);

    sqlx::query("DELETE FROM metadata_events WHERE job_id = $1")
        .bind(job.id)
        .execute(&pool)
        .await
        .expect("clean up metadata events");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("clean up metadata target");
}
