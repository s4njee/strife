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

#[tokio::test]
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
            "candidate_definition": {"version": 1, "engine": "tesseract"}
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

    let response = app
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

    sqlx::query("DELETE FROM backfill_campaigns WHERE id = $1")
        .bind(Uuid::parse_str(id).expect("UUID"))
        .execute(&pool)
        .await
        .expect("clean up campaign");
}
