//! Every route fails in the same shape.
//!
//! Before the unified error type, several endpoints returned a bare status with
//! an empty body, so a client could not write one error handler. These tests
//! pin both halves of the contract: the body is always parseable, and the
//! `code`/status pairs the frontend switches on are unchanged.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use strife_db::ROOT_NODE_ID;
use strife_storage::LocalFsBackend;
use tower::ServiceExt;
use uuid::Uuid;

async fn send(app: axum::Router, method: &str, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("read body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Every failing response must parse and carry both contract fields.
fn assert_error_shape(body: &Value, code: &str) {
    assert!(
        body.is_object(),
        "error body was not parseable JSON: {body:?}"
    );
    assert_eq!(body["code"], code, "unexpected code in {body:?}");
    assert!(
        body["message"].as_str().is_some_and(|m| !m.is_empty()),
        "error body carried no message: {body:?}"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn formerly_bare_status_endpoints_return_a_parseable_body(pool: PgPool) {
    // Each of these previously answered with an empty body, so a client had a
    // status and nothing else to act on.
    let unknown = Uuid::new_v4();

    let (status, body) = send(
        strife_api::jobs::router(pool.clone()),
        "GET",
        &format!("/api/jobs/{unknown}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_shape(&body, "not_found");

    let (status, body) = send(
        strife_api::admin::router(pool.clone()),
        "POST",
        "/api/admin/reprocess?extractor=nonsense",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_shape(&body, "bad_request");

    let (status, body) = send(
        strife_api::admin::router(pool.clone()),
        "POST",
        "/api/admin/reprocess?extractor=ocr&scope=node",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_shape(&body, "bad_request");

    let (status, body) = send(
        strife_api::search::router(pool),
        "GET",
        "/api/search?q=%20%20",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_shape(&body, "bad_request");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn success_paths_of_formerly_bare_endpoints_still_work(pool: PgPool) {
    // Story 8.5: these routes had no integration coverage at all.
    let root = std::env::temp_dir().join(format!("strife-usage-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create storage root");
    let storage = Arc::new(
        LocalFsBackend::new(root.clone())
            .await
            .expect("create backend"),
    );
    let (status, body) = send(
        strife_api::storage_usage::router(pool.clone(), storage),
        "GET",
        "/api/storage/usage",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for field in [
        "total_bytes",
        "used_bytes",
        "available_bytes",
        "originals_bytes",
        "artifacts_bytes",
        "trash_bytes",
        "usage_percent",
    ] {
        assert!(
            body[field].is_number(),
            "{field} was not numeric in {body:?}"
        );
    }

    let (status, body) = send(strife_api::jobs::router(pool.clone()), "GET", "/api/jobs").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["count"].is_number());

    let (status, body) = send(strife_api::nodes::router(pool), "GET", "/api/favorites").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["items"].is_array());

    let _ = std::fs::remove_dir_all(&root);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_known_job_reports_its_state(pool: PgPool) {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("job-{node_id}"))
        .execute(&pool)
        .await
        .expect("create node");
    let job = strife_db::enqueue_job(&pool, strife_db::JobType::MetadataExtraction, node_id, 0)
        .await
        .expect("enqueue")
        .expect("new job");

    let app = strife_api::jobs::router(pool.clone());
    let (status, body) = send(app.clone(), "GET", &format!("/api/jobs/{}", job.id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "pending");
    assert!(body["error"].is_null());

    // A job is completed from the leased state, which is what the processor
    // loop does after a successful handler run.
    strife_db::claim_job(
        &pool,
        strife_db::JobType::MetadataExtraction,
        "error-contract-test",
        chrono::Duration::minutes(1),
    )
    .await
    .expect("claim")
    .expect("leased job");
    strife_db::complete_job(&pool, job.id)
        .await
        .expect("complete");
    let (status, body) = send(app, "GET", &format!("/api/jobs/{}", job.id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "completed");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_internal_failure_never_leaks_its_cause(pool: PgPool) {
    // Dropping the table forces the handler into a database error, which is the
    // only way to observe the 500 path end to end.
    sqlx::query("DROP TABLE document_text CASCADE")
        .execute(&pool)
        .await
        .expect("drop table");

    let (status, body) = send(
        strife_api::search::router(pool),
        "GET",
        "/api/search?q=anything",
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_error_shape(&body, "internal_error");

    // The sqlx cause belongs in the logs. A client learns that it failed and
    // nothing about the schema.
    let rendered = body.to_string();
    for leak in ["document_text", "relation", "sqlx", "SELECT"] {
        assert!(
            !rendered.contains(leak),
            "error body leaked {leak}: {rendered}"
        );
    }
}
