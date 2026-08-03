use std::sync::Arc;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Duration;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use strife_api::imports::{ImportEntryResponse, ImportSourceResponse, ScanResponse};
use strife_db::{
    DEFAULT_IMPORT_SOURCE_ID, JobType, MIGRATOR, ROOT_NODE_ID, claim_job, complete_job,
};
use strife_storage::LocalFsBackend;
use strife_worker::ImportHandler;
use tower::ServiceExt;
use uuid::Uuid;

/// Cross-crate coordination key for tests that mutate the fixed import source / entries.
const IMPORT_TEST_LOCK_KEY: i64 = 0x5354_5249_4645;

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

async fn lock_import_suite(pool: &PgPool) -> Transaction<'_, Postgres> {
    let mut tx = pool.begin().await.expect("begin import suite lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(IMPORT_TEST_LOCK_KEY)
        .execute(&mut *tx)
        .await
        .expect("acquire import suite lock");
    tx
}

async fn request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = if let Some(value) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(value.to_string())
    } else {
        Body::empty()
    };
    app.oneshot(builder.body(body).expect("build request"))
        .await
        .expect("send request")
}

async fn response_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let bytes = to_bytes(response.into_body(), 128 * 1024)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse response body")
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn source_status_manual_scan_failure_filter_and_retry() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let _import_lock = lock_import_suite(&pool).await;
    let fixture = Uuid::new_v4();
    let watch_root = std::env::temp_dir().join(format!("strife-api-watch-{fixture}"));
    let storage_root = std::env::temp_dir().join(format!("strife-api-storage-{fixture}"));
    tokio::fs::create_dir_all(&watch_root)
        .await
        .expect("create watch root");
    tokio::fs::create_dir_all(&storage_root)
        .await
        .expect("create storage root");
    let storage = Arc::new(
        LocalFsBackend::new(&storage_root)
            .await
            .expect("create storage backend"),
    );
    sqlx::query("DELETE FROM jobs WHERE job_type = 'import_scan' AND import_source_id = $1")
        .bind(DEFAULT_IMPORT_SOURCE_ID)
        .execute(&pool)
        .await
        .expect("remove stale import scan jobs");
    let app = strife_api::imports::router(
        pool.clone(),
        storage.clone(),
        storage_root.clone(),
        watch_root.clone(),
        100,
    );
    strife_db::set_import_source_enabled(&pool, DEFAULT_IMPORT_SOURCE_ID, true)
        .await
        .expect("enable source");

    let listed = request(app.clone(), "GET", "/api/import-sources", None).await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed: Vec<ImportSourceResponse> = response_json(listed).await;
    let source = listed
        .iter()
        .find(|source| source.id == DEFAULT_IMPORT_SOURCE_ID)
        .expect("fixed source listed");
    assert_eq!(source.watch_path, "/mnt/ext/watch");

    let disabled = request(
        app.clone(),
        "PATCH",
        &format!("/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}"),
        Some(json!({"enabled": false})),
    )
    .await;
    assert_eq!(disabled.status(), StatusCode::OK);
    let rejected_scan = request(
        app.clone(),
        "POST",
        &format!("/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}/scan"),
        None,
    )
    .await;
    assert_eq!(rejected_scan.status(), StatusCode::CONFLICT);
    request(
        app.clone(),
        "PATCH",
        &format!("/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}"),
        Some(json!({"enabled": true})),
    )
    .await;

    let good_name = format!("api-import-{fixture}.txt");
    tokio::fs::write(watch_root.join(&good_name), b"import me")
        .await
        .expect("write import source");
    let scan = request(
        app.clone(),
        "POST",
        &format!("/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}/scan"),
        None,
    )
    .await;
    assert_eq!(scan.status(), StatusCode::ACCEPTED);
    let scan: ScanResponse = response_json(scan).await;
    assert_eq!(scan.status, "pending");
    assert!(watch_root.join(&good_name).exists());
    let repeated_scan = request(
        app.clone(),
        "POST",
        &format!("/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}/scan"),
        None,
    )
    .await;
    assert_eq!(repeated_scan.status(), StatusCode::ACCEPTED);
    let repeated_scan: ScanResponse = response_json(repeated_scan).await;
    assert_eq!(repeated_scan.job_id, scan.job_id);
    let scan_job = claim_job(
        &pool,
        JobType::ImportScan,
        "imports-api-test",
        Duration::minutes(5),
    )
    .await
    .expect("claim queued scan")
    .expect("scan job queued");
    assert_eq!(scan_job.id, scan.job_id);
    ImportHandler::new(
        pool.clone(),
        storage.clone(),
        storage_root.clone(),
        watch_root.clone(),
        100,
    )
    .scan(&scan_job)
    .await
    .expect("process queued scan");
    complete_job(&pool, scan_job.id)
        .await
        .expect("complete scan job")
        .expect("leased scan completed");
    assert!(!watch_root.join(&good_name).exists());

    let conflict_name = format!("api-conflict-{fixture}.txt");
    let conflict_node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(conflict_node_id)
        .bind(ROOT_NODE_ID)
        .bind(&conflict_name)
        .execute(&pool)
        .await
        .expect("create conflicting node");
    tokio::fs::write(watch_root.join(&conflict_name), b"conflict")
        .await
        .expect("write conflicting source");
    let conflict_scan = request(
        app.clone(),
        "POST",
        &format!("/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}/scan"),
        None,
    )
    .await;
    assert_eq!(conflict_scan.status(), StatusCode::ACCEPTED);
    let conflict_scan: ScanResponse = response_json(conflict_scan).await;
    let conflict_job = claim_job(
        &pool,
        JobType::ImportScan,
        "imports-api-test",
        Duration::minutes(5),
    )
    .await
    .expect("claim conflict scan")
    .expect("conflict scan queued");
    assert_eq!(conflict_job.id, conflict_scan.job_id);
    ImportHandler::new(
        pool.clone(),
        storage,
        storage_root.clone(),
        watch_root.clone(),
        100,
    )
    .scan(&conflict_job)
    .await
    .expect("process conflict scan");
    complete_job(&pool, conflict_job.id)
        .await
        .expect("complete conflict scan job")
        .expect("leased conflict scan completed");
    let failures = request(
        app.clone(),
        "GET",
        &format!("/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}/entries?state=failed"),
        None,
    )
    .await;
    let failures: Vec<ImportEntryResponse> = response_json(failures).await;
    let failure = failures
        .iter()
        .find(|entry| entry.source_path == conflict_name)
        .expect("conflict failure listed");
    assert!(failure.error_message.is_some());
    let retried = request(
        app,
        "POST",
        &format!(
            "/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}/entries/{}/retry",
            failure.id
        ),
        None,
    )
    .await;
    assert_eq!(retried.status(), StatusCode::OK);
    let retried: ImportEntryResponse = response_json(retried).await;
    assert_eq!(retried.state, "discovered");

    tokio::fs::remove_dir_all(&watch_root)
        .await
        .expect("remove watch fixture");
    tokio::fs::remove_dir_all(&storage_root)
        .await
        .expect("remove storage fixture");
}

#[tokio::test]
async fn scan_rejects_watch_and_storage_overlap() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _import_lock = lock_import_suite(&pool).await;
    let root = std::env::temp_dir().join(format!("strife-api-overlap-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&root)
        .await
        .expect("create overlapping root");
    let storage = Arc::new(
        LocalFsBackend::new(&root)
            .await
            .expect("create storage backend"),
    );
    strife_db::set_import_source_enabled(&pool, DEFAULT_IMPORT_SOURCE_ID, true)
        .await
        .expect("enable source");
    let app = strife_api::imports::router(pool.clone(), storage, root.clone(), root.clone(), 100);
    let response = request(
        app,
        "POST",
        &format!("/api/import-sources/{DEFAULT_IMPORT_SOURCE_ID}/scan"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove overlap fixture");
}
