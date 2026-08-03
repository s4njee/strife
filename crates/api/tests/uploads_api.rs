use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Duration;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_api::uploads::CreateUploadResponse;
use strife_db::{MIGRATOR, ROOT_NODE_ID};
use strife_storage::{DiskUsage, LocalFsBackend, StorageBackend, StorageKey, StorageReader};
use tokio::{fs, io::AsyncReadExt};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct CapacityStorage {
    usage: DiskUsage,
}

#[async_trait]
impl StorageBackend for CapacityStorage {
    async fn put_stream(&self, _key: StorageKey, _reader: StorageReader) -> Result<()> {
        Ok(())
    }

    async fn write_range(
        &self,
        _key: StorageKey,
        _offset: u64,
        _reader: StorageReader,
    ) -> Result<u64> {
        bail!("not used")
    }

    async fn move_object(&self, _source: StorageKey, _destination: StorageKey) -> Result<()> {
        bail!("not used")
    }

    async fn detect_mime(&self, _key: StorageKey) -> Result<String> {
        bail!("not used")
    }

    async fn get_stream(&self, _key: StorageKey) -> Result<StorageReader> {
        bail!("not used")
    }

    async fn get_range(
        &self,
        _key: StorageKey,
        _offset: u64,
        _length: u64,
    ) -> Result<StorageReader> {
        bail!("not used")
    }

    async fn delete(&self, _key: StorageKey) -> Result<()> {
        Ok(())
    }

    async fn exists(&self, _key: StorageKey) -> Result<bool> {
        bail!("not used")
    }

    async fn disk_usage(&self) -> Result<DiskUsage> {
        Ok(self.usage)
    }
}

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("connect to integration-test PostgreSQL");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    Some(pool)
}

async fn create_fixture_folder(pool: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder')")
        .bind(id)
        .bind(ROOT_NODE_ID)
        .bind(format!("upload-api-test-{id}"))
        .execute(pool)
        .await
        .expect("create fixture folder");
    id
}

async fn json_request(app: axum::Router, body: Value) -> axum::response::Response {
    app.oneshot(
        Request::post("/api/uploads")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("build request"),
    )
    .await
    .expect("send request")
}

async fn chunk_request(
    app: axum::Router,
    session_id: Uuid,
    content_range: &str,
    body: &'static [u8],
) -> axum::response::Response {
    app.oneshot(
        Request::patch(format!("/api/uploads/{session_id}"))
            .header("content-range", content_range)
            .body(Body::from(body))
            .expect("build chunk request"),
    )
    .await
    .expect("send chunk request")
}

async fn finalize_request(app: axum::Router, session_id: Uuid) -> axum::response::Response {
    app.oneshot(
        Request::post(format!("/api/uploads/{session_id}/finalize"))
            .body(Body::empty())
            .expect("build finalize request"),
    )
    .await
    .expect("send finalize request")
}

async fn cancel_request(app: axum::Router, session_id: Uuid) -> axum::response::Response {
    app.oneshot(
        Request::delete(format!("/api/uploads/{session_id}"))
            .body(Body::empty())
            .expect("build cancel request"),
    )
    .await
    .expect("send cancel request")
}

async fn progress_request(app: axum::Router, session_id: Uuid) -> axum::response::Response {
    app.oneshot(
        Request::get(format!("/api/uploads/{session_id}"))
            .body(Body::empty())
            .expect("build progress request"),
    )
    .await
    .expect("send progress request")
}

async fn list_uploads_request(app: axum::Router, folder_id: Uuid) -> axum::response::Response {
    app.oneshot(
        Request::get(format!("/api/uploads?folder_id={folder_id}"))
            .body(Body::empty())
            .expect("build list request"),
    )
    .await
    .expect("send list request")
}

async fn response_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse response body")
}

fn storage(used_bytes: u64) -> Arc<dyn StorageBackend> {
    Arc::new(CapacityStorage {
        usage: DiskUsage {
            total_bytes: 1_000,
            used_bytes,
            available_bytes: 1_000 - used_bytes,
        },
    })
}

async fn cleanup_upload_fixture(pool: &PgPool, folder_id: Uuid, root: &std::path::Path) {
    sqlx::query("DELETE FROM upload_sessions WHERE target_folder_id = $1")
        .bind(folder_id)
        .execute(pool)
        .await
        .expect("remove sessions");
    sqlx::query(
        "DELETE FROM file_objects WHERE node_id IN (SELECT id FROM nodes WHERE parent_id = $1)",
    )
    .bind(folder_id)
    .execute(pool)
    .await
    .expect("remove objects");
    sqlx::query("DELETE FROM nodes WHERE parent_id = $1")
        .bind(folder_id)
        .execute(pool)
        .await
        .expect("remove child nodes");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(folder_id)
        .execute(pool)
        .await
        .expect("remove fixture folder");
    fs::remove_dir_all(root).await.expect("remove storage");
}

#[tokio::test]
async fn upload_initiation_validates_names_capacity_and_expiry() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let folder_id = create_fixture_folder(&pool).await;
    let app = strife_api::uploads::router(pool.clone(), storage(100), Duration::hours(24), 90);
    let request = json!({
        "folder_id": folder_id,
        "name": "video.bin",
        "size": 100,
        "source_created_at": null,
        "source_modified_at": null
    });

    let created = json_request(app.clone(), request.clone()).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: CreateUploadResponse = response_json(created).await;
    let progress = strife_db::get_session_progress(&pool, created.session_id)
        .await
        .expect("load created session")
        .expect("session exists");
    assert_eq!(
        progress.session.staging_key,
        created.staging_key.simple().to_string()
    );
    assert!(progress.session.expires_at > chrono::Utc::now() + Duration::hours(23));

    assert_eq!(
        json_request(app.clone(), request).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        json_request(
            app,
            json!({"folder_id": Uuid::new_v4(), "name": "missing", "size": null})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let disk_full_app =
        strife_api::uploads::router(pool.clone(), storage(910), Duration::hours(24), 90);
    let disk_full = json_request(
        disk_full_app,
        json!({"folder_id": folder_id, "name": "large.bin", "size": 0}),
    )
    .await;
    assert_eq!(disk_full.status(), StatusCode::INSUFFICIENT_STORAGE);
    let disk_full: Value = response_json(disk_full).await;
    assert_eq!(disk_full["code"], "disk_full");
    assert_eq!(disk_full["error"], "disk_full");
    assert_eq!(disk_full["usage_percent"], 91);

    sqlx::query("DELETE FROM upload_sessions WHERE target_folder_id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .expect("remove sessions");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .expect("remove fixture folder");
}

#[tokio::test]
async fn chunks_stream_out_of_order_and_reject_overlaps() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let folder_id = create_fixture_folder(&pool).await;
    let root = std::env::temp_dir().join(format!("strife-upload-api-{}", Uuid::new_v4()));
    let backend = Arc::new(LocalFsBackend::new(&root).await.expect("create storage"));
    let app = strife_api::uploads::router(pool.clone(), backend.clone(), Duration::hours(24), 90);
    let created = json_request(
        app.clone(),
        json!({"folder_id": folder_id, "name": "chunks.bin", "size": 10}),
    )
    .await;
    let created: CreateUploadResponse = response_json(created).await;

    let later = chunk_request(app.clone(), created.session_id, "bytes 5-9/10", b"world").await;
    assert_eq!(later.status(), StatusCode::OK);
    let later: Value = response_json(later).await;
    assert_eq!(later["received_bytes"], 5);
    assert_eq!(later["complete"], false);

    let earlier = chunk_request(app.clone(), created.session_id, "bytes 0-4/10", b"hello").await;
    assert_eq!(earlier.status(), StatusCode::OK);
    let earlier: Value = response_json(earlier).await;
    assert_eq!(earlier["received_bytes"], 10);
    assert_eq!(earlier["complete"], true);
    let progress: Value =
        response_json(progress_request(app.clone(), created.session_id).await).await;
    assert_eq!(progress["session_id"], created.session_id.to_string());
    assert_eq!(progress["state"], "active");
    assert_eq!(progress["display_name"], "chunks.bin");
    assert_eq!(progress["received_bytes"], 10);
    assert_eq!(progress["expected_bytes"], 10);
    assert_eq!(
        progress["received_ranges"][0],
        json!({"start": 0, "end": 4})
    );
    assert_eq!(
        progress["received_ranges"][1],
        json!({"start": 5, "end": 9})
    );
    assert!(progress["created_at"].is_string());
    assert!(progress["expires_at"].is_string());
    let active: Vec<Value> =
        response_json(list_uploads_request(app.clone(), folder_id).await).await;
    assert!(
        active
            .iter()
            .any(|session| session["session_id"] == created.session_id.to_string())
    );
    assert_eq!(
        chunk_request(app.clone(), created.session_id, "bytes 3-6/10", b"xxxx")
            .await
            .status(),
        StatusCode::CONFLICT
    );
    strife_db::finalize_session(&pool, created.session_id, "pending-finalization-test", None)
        .await
        .expect("mark session completed");
    let active: Vec<Value> =
        response_json(list_uploads_request(app.clone(), folder_id).await).await;
    assert!(
        active
            .iter()
            .all(|session| session["session_id"] != created.session_id.to_string())
    );
    assert_eq!(
        chunk_request(app, created.session_id, "bytes 0-4/10", b"hello")
            .await
            .status(),
        StatusCode::GONE
    );

    let mut stored = backend
        .get_stream(StorageKey::staging(created.staging_key))
        .await
        .expect("open staged upload");
    let mut bytes = Vec::new();
    stored.read_to_end(&mut bytes).await.expect("read upload");
    assert_eq!(bytes, b"helloworld");

    sqlx::query("DELETE FROM upload_sessions WHERE target_folder_id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .expect("remove sessions");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .expect("remove fixture folder");
    fs::remove_dir_all(root).await.expect("remove storage");
}

#[tokio::test]
async fn finalization_is_atomic_content_aware_and_idempotent() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let folder_id = create_fixture_folder(&pool).await;
    let root = std::env::temp_dir().join(format!("strife-finalize-api-{}", Uuid::new_v4()));
    let backend = Arc::new(LocalFsBackend::new(&root).await.expect("create storage"));
    let app = strife_api::uploads::router(pool.clone(), backend.clone(), Duration::hours(24), 90);
    let source_modified_at = "2025-02-03T04:05:06Z";
    let created = json_request(
        app.clone(),
        json!({
            "folder_id": folder_id,
            "name": "misleading.jpg",
            "size": 11,
            "source_created_at": "2025-01-02T03:04:05Z",
            "source_modified_at": source_modified_at
        }),
    )
    .await;
    let created: CreateUploadResponse = response_json(created).await;
    assert_eq!(
        chunk_request(
            app.clone(),
            created.session_id,
            "bytes 0-10/11",
            b"hello world"
        )
        .await
        .status(),
        StatusCode::OK
    );

    let finalized = finalize_request(app.clone(), created.session_id).await;
    assert_eq!(finalized.status(), StatusCode::OK);
    let finalized: Value = response_json(finalized).await;
    let node_id =
        Uuid::parse_str(finalized["id"].as_str().expect("node id")).expect("valid node id");
    let node = strife_db::get_node_by_id(&pool, node_id)
        .await
        .expect("load node")
        .expect("node exists");
    assert_eq!(
        node.source_modified_at
            .expect("source modified")
            .to_rfc3339(),
        "2025-02-03T04:05:06+00:00"
    );
    let object = strife_db::get_file_object_by_node_id(&pool, node_id)
        .await
        .expect("load object")
        .expect("object exists");
    assert_eq!(object.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(
        object.checksum_sha256.as_deref(),
        Some("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9")
    );
    let original_id = Uuid::parse_str(&object.storage_key).expect("original key");
    assert!(
        backend
            .exists(StorageKey::original(original_id))
            .await
            .expect("check original")
    );
    assert!(
        !backend
            .exists(StorageKey::staging(created.staging_key))
            .await
            .expect("check staging")
    );
    let job_counts: (i64, i64) = sqlx::query_as(
        r"
        SELECT
            count(*) FILTER (WHERE job_type = 'metadata_extraction'),
            count(*) FILTER (WHERE job_type = 'ocr')
        FROM jobs WHERE target_node_id = $1
        ",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("count jobs");
    assert_eq!(job_counts, (1, 1));

    let repeated: Value =
        response_json(finalize_request(app.clone(), created.session_id).await).await;
    assert_eq!(repeated["id"], finalized["id"]);

    let incomplete = json_request(
        app.clone(),
        json!({"folder_id": folder_id, "name": "incomplete.bin", "size": 10}),
    )
    .await;
    let incomplete: CreateUploadResponse = response_json(incomplete).await;
    chunk_request(app.clone(), incomplete.session_id, "bytes 0-4/10", b"short").await;
    assert_eq!(
        finalize_request(app, incomplete.session_id).await.status(),
        StatusCode::BAD_REQUEST
    );

    cleanup_upload_fixture(&pool, folder_id, &root).await;
}

#[tokio::test]
async fn finalization_rechecks_name_conflicts_and_restores_staging() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let folder_id = create_fixture_folder(&pool).await;
    let root = std::env::temp_dir().join(format!("strife-finalize-race-{}", Uuid::new_v4()));
    let backend = Arc::new(LocalFsBackend::new(&root).await.expect("create storage"));
    let app = strife_api::uploads::router(pool.clone(), backend.clone(), Duration::hours(24), 90);
    let created = json_request(
        app.clone(),
        json!({"folder_id": folder_id, "name": "raced.txt", "size": 5}),
    )
    .await;
    let created: CreateUploadResponse = response_json(created).await;
    chunk_request(app.clone(), created.session_id, "bytes 0-4/5", b"hello").await;

    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(Uuid::new_v4())
        .bind(folder_id)
        .bind("raced.txt")
        .execute(&pool)
        .await
        .expect("create competing node");

    let conflict = finalize_request(app, created.session_id).await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert!(
        backend
            .exists(StorageKey::staging(created.staging_key))
            .await
            .expect("check restored staging object")
    );
    let progress = strife_db::get_session_progress(&pool, created.session_id)
        .await
        .expect("load session")
        .expect("session exists");
    assert_eq!(
        progress.session.state,
        strife_db::UploadSessionState::Active
    );

    cleanup_upload_fixture(&pool, folder_id, &root).await;
}

#[tokio::test]
async fn cancellation_and_expiry_cleanup_remove_staging_objects_idempotently() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let folder_id = create_fixture_folder(&pool).await;
    let root = std::env::temp_dir().join(format!("strife-cleanup-api-{}", Uuid::new_v4()));
    let backend = Arc::new(LocalFsBackend::new(&root).await.expect("create storage"));
    let app = strife_api::uploads::router(pool.clone(), backend.clone(), Duration::hours(24), 90);
    let cancelled: CreateUploadResponse = response_json(
        json_request(
            app.clone(),
            json!({"folder_id": folder_id, "name": "cancel.bin", "size": 5}),
        )
        .await,
    )
    .await;
    chunk_request(app.clone(), cancelled.session_id, "bytes 0-4/5", b"hello").await;

    assert_eq!(
        cancel_request(app.clone(), cancelled.session_id)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        cancel_request(app.clone(), cancelled.session_id)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        !backend
            .exists(StorageKey::staging(cancelled.staging_key))
            .await
            .expect("check cancelled staging")
    );

    let expired: CreateUploadResponse = response_json(
        json_request(
            app,
            json!({"folder_id": folder_id, "name": "expired.bin", "size": 1}),
        )
        .await,
    )
    .await;
    sqlx::query(
        "UPDATE upload_sessions SET expires_at = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(expired.session_id)
    .execute(&pool)
    .await
    .expect("expire session fixture");
    assert_eq!(
        strife_api::uploads::cleanup_expired_uploads(&pool, backend.as_ref())
            .await
            .expect("run cleanup"),
        1
    );
    assert_eq!(
        strife_api::uploads::cleanup_expired_uploads(&pool, backend.as_ref())
            .await
            .expect("repeat cleanup"),
        0
    );
    assert!(
        !backend
            .exists(StorageKey::staging(expired.staging_key))
            .await
            .expect("check expired staging")
    );
    assert_eq!(
        strife_db::get_upload_session(&pool, expired.session_id)
            .await
            .expect("load expired session")
            .expect("expired session exists")
            .state,
        strife_db::UploadSessionState::Expired
    );

    cleanup_upload_fixture(&pool, folder_id, &root).await;
}
