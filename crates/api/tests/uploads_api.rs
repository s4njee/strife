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
        bail!("not used")
    }

    async fn write_range(
        &self,
        _key: StorageKey,
        _offset: u64,
        _reader: StorageReader,
    ) -> Result<u64> {
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
        bail!("not used")
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
        strife_api::uploads::router(pool.clone(), storage(850), Duration::hours(24), 90);
    let disk_full = json_request(
        disk_full_app,
        json!({"folder_id": folder_id, "name": "large.bin", "size": 100}),
    )
    .await;
    assert_eq!(disk_full.status(), StatusCode::INSUFFICIENT_STORAGE);
    let disk_full: Value = response_json(disk_full).await;
    assert_eq!(disk_full["code"], "disk_full");

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
    assert_eq!(
        chunk_request(app.clone(), created.session_id, "bytes 3-6/10", b"xxxx")
            .await
            .status(),
        StatusCode::CONFLICT
    );
    strife_db::finalize_session(&pool, created.session_id, "pending-finalization-test", None)
        .await
        .expect("mark session completed");
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
