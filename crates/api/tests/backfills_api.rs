use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Utc;
use futures_util::StreamExt;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::MIGRATOR;
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

async fn json_request(
    app: axum::Router,
    method: &str,
    uri: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    let body = if let Some(payload) = payload {
        request = request.header("content-type", "application/json");
        Body::from(payload.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .oneshot(request.body(body).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("response body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON body")
    };
    (status, value)
}

fn approved_canary(stage: i64) -> Value {
    json!({
        "stage": stage,
        "processed": stage,
        "failed": 2,
        "duration_seconds": 3600.0,
        "p50_seconds": 21.0,
        "p95_seconds": 42.0,
        "peak_cpu_percent": 88.0,
        "peak_memory_bytes": 536_870_912,
        "peak_temperature_c": 71.0,
        "peak_io_wait_percent": 4.0,
        "database_growth_bytes": 1_048_576,
        "approved": true
    })
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn campaign_api_requires_explicit_prepare_and_resume_and_streams_audit_events() {
    let Some(pool) = test_pool().await else {
        return;
    };
    // Only one heavy campaign may run at a time, and this suite shares its
    // database. Quiesce any campaign left running by another test before
    // asserting that this one can be resumed.
    sqlx::query(
        r"
        UPDATE backfill_campaigns SET state = 'paused', paused_at = now()
        WHERE resource_class = 'heavy_cpu' AND state IN ('running', 'draining')
        ",
    )
    .execute(&pool)
    .await
    .expect("quiesce competing heavy campaigns");

    let app = strife_api::backfills::router(pool.clone());
    let (invalid_status, _) = json_request(
        app.clone(),
        "POST",
        "/api/backfills",
        Some(json!({"kind": "ocr", "resource_class": "heavy_cpu"})),
    )
    .await;
    assert_eq!(invalid_status, StatusCode::UNPROCESSABLE_ENTITY);

    let (created_status, created) = json_request(
        app.clone(),
        "POST",
        "/api/backfills",
        Some(json!({
            "kind": "ocr",
            "resource_class": "heavy_cpu",
            "candidate_definition": {
                "version": 1,
                "engine": "tesseract",
                "canary_limit": 100
            }
        })),
    )
    .await;
    assert_eq!(created_status, StatusCode::CREATED);
    assert_eq!(created["state"], "draft");
    let id = created["id"].as_str().expect("campaign id");

    let (prepare_status, prepared) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/prepare"),
        Some(json!({
            "candidate_count": 700_000,
            "snapshot_before": Utc::now(),
            "reason": "preflight confirmed"
        })),
    )
    .await;
    assert_eq!(prepare_status, StatusCode::OK);
    assert_eq!(prepared["state"], "paused");

    let (resume_status, resumed) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/actions"),
        Some(json!({"action": "resume", "reason": "operator canary"})),
    )
    .await;
    assert_eq!(resume_status, StatusCode::OK);
    assert_eq!(resumed["state"], "running");

    let (metrics_status, metrics) = json_request(
        app.clone(),
        "GET",
        &format!("/api/backfills/{id}/metrics"),
        None,
    )
    .await;
    assert_eq!(metrics_status, StatusCode::OK);
    assert_eq!(metrics["pending"], 0);
    assert_eq!(metrics["running"], 0);
    assert_eq!(metrics["remaining"], 700_000);
    assert!(metrics["throughput_per_hour"].is_null());

    let canary = approved_canary(100);
    let (running_status, _) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-results"),
        Some(canary.clone()),
    )
    .await;
    assert_eq!(running_status, StatusCode::CONFLICT);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/backfills/events?campaign_id={id}"))
                .header("last-event-id", "0")
                .body(Body::empty())
                .expect("SSE request"),
        )
        .await
        .expect("SSE response");
    let mut stream = response.into_body().into_data_stream();
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .expect("SSE timeout")
        .expect("SSE event")
        .expect("SSE bytes");
    assert!(String::from_utf8_lossy(&chunk).contains("event: campaign"));
    drop(stream);

    let (pause_status, paused) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/actions"),
        Some(json!({"action": "pause", "reason": "canary drained"})),
    )
    .await;
    assert_eq!(pause_status, StatusCode::OK);
    assert_eq!(paused["state"], "paused");
    sqlx::query(
        "UPDATE backfill_campaigns SET enqueued_count = 100, completed_count = 98, \
         failed_count = 2 WHERE id = $1",
    )
    .bind(Uuid::parse_str(id).expect("UUID"))
    .execute(&pool)
    .await
    .expect("simulate drained canary outcomes");
    let (unapproved_status, _) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-stage"),
        Some(json!({"next_stage": "1000", "reason": "reviewed metrics"})),
    )
    .await;
    assert_eq!(unapproved_status, StatusCode::CONFLICT);
    let (record_status, recorded) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-results"),
        Some(canary),
    )
    .await;
    assert_eq!(record_status, StatusCode::CREATED);
    assert_eq!(recorded["details"]["stage"], 100);
    assert_eq!(recorded["details"]["throughput_per_hour"], 100.0);
    let (skip_status, _) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-stage"),
        Some(json!({"next_stage": "10000", "reason": "skip a stage"})),
    )
    .await;
    assert_eq!(skip_status, StatusCode::CONFLICT);
    let active_job = Uuid::new_v4();
    sqlx::query(
        r"
        INSERT INTO jobs
            (id, job_type, target_node_id, priority, origin, campaign_id, resource_class)
        VALUES ($1, 'ocr', $2, -100, 'backfill', $3, 'heavy_cpu')
        ",
    )
    .bind(active_job)
    .bind(strife_db::ROOT_NODE_ID)
    .bind(Uuid::parse_str(id).expect("UUID"))
    .execute(&pool)
    .await
    .expect("seed active campaign job");
    let (active_status, _) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-stage"),
        Some(json!({"next_stage": "1000", "reason": "job still active"})),
    )
    .await;
    assert_eq!(active_status, StatusCode::CONFLICT);
    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(active_job)
        .execute(&pool)
        .await
        .expect("remove active campaign job");
    let (advance_status, advanced) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-stage"),
        Some(json!({"next_stage": "1000", "reason": "metrics approved"})),
    )
    .await;
    assert_eq!(advance_status, StatusCode::OK);
    assert_eq!(advanced["candidate_definition"]["canary_limit"], 1_000);
    sqlx::query(
        "UPDATE backfill_campaigns SET enqueued_count = 1000, completed_count = 998, \
         failed_count = 2 WHERE id = $1",
    )
    .bind(Uuid::parse_str(id).expect("UUID"))
    .execute(&pool)
    .await
    .expect("simulate 1000-file canary outcomes");
    let (record_status, _) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-results"),
        Some(approved_canary(1_000)),
    )
    .await;
    assert_eq!(record_status, StatusCode::CREATED);
    let (advance_status, advanced) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-stage"),
        Some(json!({"next_stage": "10000", "reason": "second stage approved"})),
    )
    .await;
    assert_eq!(advance_status, StatusCode::OK);
    assert_eq!(advanced["candidate_definition"]["canary_limit"], 10_000);
    sqlx::query(
        "UPDATE backfill_campaigns SET enqueued_count = 10000, completed_count = 9998, \
         failed_count = 2 WHERE id = $1",
    )
    .bind(Uuid::parse_str(id).expect("UUID"))
    .execute(&pool)
    .await
    .expect("simulate 10000-file canary outcomes");
    let (record_status, _) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-results"),
        Some(approved_canary(10_000)),
    )
    .await;
    assert_eq!(record_status, StatusCode::CREATED);
    let (full_status, full) = json_request(
        app.clone(),
        "POST",
        &format!("/api/backfills/{id}/canary-stage"),
        Some(json!({"next_stage": "full", "reason": "full OCR authorized"})),
    )
    .await;
    assert_eq!(full_status, StatusCode::OK);
    assert!(full["candidate_definition"].get("canary_limit").is_none());
    let (results_status, results) = json_request(
        app,
        "GET",
        &format!("/api/backfills/{id}/canary-results"),
        None,
    )
    .await;
    assert_eq!(results_status, StatusCode::OK);
    assert_eq!(results.as_array().expect("canary results").len(), 3);

    sqlx::query("DELETE FROM backfill_campaigns WHERE id = $1")
        .bind(Uuid::parse_str(id).expect("UUID"))
        .execute(&pool)
        .await
        .expect("clean up campaign");
}
