//! Edge-case and failure-mode coverage for v1 stabilization (Story 7.3).

use std::{
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Duration as ChronoDuration;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    JobType, MIGRATOR, ROOT_NODE_ID, claim_job, complete_job, enqueue_expired_trash_deletions,
    enqueue_job, get_node_by_id, trash_node,
};
use strife_storage::{DiskUsage, LocalFsBackend, StorageBackend, StorageKey, StorageReader};
use strife_worker::{DeletionService, JobHandler, MetadataHandler};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct CapacityStorage {
    usage: DiskUsage,
    inner: Arc<LocalFsBackend>,
}

#[async_trait]
impl StorageBackend for CapacityStorage {
    async fn put_stream(&self, key: StorageKey, reader: StorageReader) -> Result<()> {
        self.inner.put_stream(key, reader).await
    }
    async fn write_range(
        &self,
        key: StorageKey,
        offset: u64,
        reader: StorageReader,
    ) -> Result<u64> {
        self.inner.write_range(key, offset, reader).await
    }
    async fn move_object(&self, source: StorageKey, destination: StorageKey) -> Result<()> {
        self.inner.move_object(source, destination).await
    }
    async fn detect_mime(&self, key: StorageKey) -> Result<String> {
        self.inner.detect_mime(key).await
    }
    async fn get_stream(&self, key: StorageKey) -> Result<StorageReader> {
        self.inner.get_stream(key).await
    }
    async fn get_range(&self, key: StorageKey, offset: u64, length: u64) -> Result<StorageReader> {
        self.inner.get_range(key, offset, length).await
    }
    async fn delete(&self, key: StorageKey) -> Result<()> {
        self.inner.delete(key).await
    }
    async fn exists(&self, key: StorageKey) -> Result<bool> {
        self.inner.exists(key).await
    }
    async fn disk_usage(&self) -> Result<DiskUsage> {
        Ok(self.usage)
    }
}

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    Some(pool)
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn disk_guard_rejects_upload_at_91_percent() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL unset; skipping");
        return;
    };
    let folder_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder')")
        .bind(folder_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("edge-disk-{folder_id}"))
        .execute(&pool)
        .await
        .expect("folder");
    let storage = Arc::new(CapacityStorage {
        usage: DiskUsage {
            total_bytes: 1000,
            used_bytes: 910,
            available_bytes: 90,
        },
        inner: Arc::new(
            LocalFsBackend::new(std::env::temp_dir().join(format!("edge-disk-{folder_id}")))
                .await
                .expect("storage"),
        ),
    });
    let app = strife_api::uploads::router(pool.clone(), storage, ChronoDuration::hours(1), 90);
    let response = app
        .oneshot(
            Request::post("/api/uploads")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"folder_id": folder_id, "name": "big.bin", "size": 1}).to_string(),
                ))
                .expect("req"),
        )
        .await
        .expect("send");
    assert_eq!(response.status(), StatusCode::INSUFFICIENT_STORAGE);
    let body = response_json(response).await;
    assert_eq!(body["error"], "disk_full");
    assert_eq!(body["usage_percent"], 91);
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn api_fails_fast_on_unreachable_postgres_and_missing_storage() {
    // Unreachable PostgreSQL: connect helper times out / errors.
    let result = strife_api::connect_database("postgresql://strife:bad@127.0.0.1:1/strife").await;
    assert!(result.is_err(), "unreachable database must fail connect");

    let missing = std::env::temp_dir().join(format!("strife-missing-{}", Uuid::new_v4()));
    let result = strife_api::verify_storage_root(&missing);
    assert!(
        result.is_err(),
        "missing STORAGE_ROOT must fail verification"
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn interrupted_upload_resumes_remaining_chunks() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL unset; skipping");
        return;
    };
    let storage_root = std::env::temp_dir().join(format!("edge-resume-{}", Uuid::new_v4()));
    let storage = Arc::new(LocalFsBackend::new(&storage_root).await.expect("storage"));
    let folder_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder')")
        .bind(folder_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("edge-resume-{folder_id}"))
        .execute(&pool)
        .await
        .expect("folder");

    let content = b"chunk0chunk1chunk2chunk3";
    let app =
        strife_api::uploads::router(pool.clone(), storage.clone(), ChronoDuration::hours(1), 90);

    let created = app
        .clone()
        .oneshot(
            Request::post("/api/uploads")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "folder_id": folder_id,
                        "name": "resume.bin",
                        "size": content.len()
                    })
                    .to_string(),
                ))
                .expect("create"),
        )
        .await
        .expect("send");
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value = response_json(created).await;
    let session_id: Uuid = created["session_id"].as_str().unwrap().parse().unwrap();

    // Upload first 3 chunks (bytes 0-5, 6-11, 12-17), "kill" by dropping app and rebuilding.
    for (i, slice) in content.chunks(6).take(3).enumerate() {
        let start = i * 6;
        let end = start + slice.len() - 1;
        let response = app
            .clone()
            .oneshot(
                Request::patch(format!("/api/uploads/{session_id}"))
                    .header(
                        "content-range",
                        format!("bytes {start}-{end}/{}", content.len()),
                    )
                    .body(Body::from(slice.to_vec()))
                    .expect("chunk"),
            )
            .await
            .expect("send chunk");
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Simulate restart with a fresh router sharing pool+storage
    let app2 =
        strife_api::uploads::router(pool.clone(), storage.clone(), ChronoDuration::hours(1), 90);
    let progress = app2
        .clone()
        .oneshot(
            Request::get(format!("/api/uploads/{session_id}"))
                .body(Body::empty())
                .expect("progress"),
        )
        .await
        .expect("get progress");
    assert_eq!(progress.status(), StatusCode::OK);
    let progress: Value = response_json(progress).await;
    assert_eq!(progress["received_bytes"], 18);

    // Resume final chunk
    let response = app2
        .clone()
        .oneshot(
            Request::patch(format!("/api/uploads/{session_id}"))
                .header("content-range", format!("bytes 18-23/{}", content.len()))
                .body(Body::from(content[18..].to_vec()))
                .expect("final chunk"),
        )
        .await
        .expect("send final");
    assert_eq!(response.status(), StatusCode::OK);
    let finalized = app2
        .oneshot(
            Request::post(format!("/api/uploads/{session_id}/finalize"))
                .body(Body::empty())
                .expect("finalize"),
        )
        .await
        .expect("send finalize");
    assert_eq!(finalized.status(), StatusCode::OK);

    let _ = tokio::fs::remove_dir_all(storage_root).await;
    sqlx::query("DELETE FROM upload_sessions WHERE target_folder_id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query(
        "DELETE FROM file_objects WHERE node_id IN (SELECT id FROM nodes WHERE parent_id = $1)",
    )
    .bind(folder_id)
    .execute(&pool)
    .await
    .ok();
    sqlx::query("DELETE FROM nodes WHERE parent_id = $1 OR id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn malformed_file_metadata_fails_gracefully_and_file_stays_accessible() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL unset; skipping");
        return;
    };
    let storage_root = std::env::temp_dir().join(format!("edge-malformed-{}", Uuid::new_v4()));
    let storage = Arc::new(LocalFsBackend::new(&storage_root).await.expect("storage"));
    let node_id = Uuid::new_v4();
    let storage_id = Uuid::new_v4();
    // Corrupt JPEG-like header
    let bytes = b"\xff\xd8\xff not a real jpeg payload \x00\x00 garbage";
    storage
        .put_stream(
            StorageKey::original(storage_id),
            Box::pin(std::io::Cursor::new(bytes.to_vec())),
        )
        .await
        .expect("store");
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("malformed-{node_id}.jpg"))
        .execute(&pool)
        .await
        .expect("node");
    sqlx::query(
        r"
        INSERT INTO file_objects (id, node_id, storage_key, byte_size, mime_type, upload_state)
        VALUES ($1, $2, $3, $4, 'image/jpeg', 'finalized')
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(storage_id.simple().to_string())
    .bind(i64::try_from(bytes.len()).unwrap())
    .execute(&pool)
    .await
    .expect("object");

    enqueue_job(&pool, JobType::MetadataExtraction, node_id, 0)
        .await
        .expect("enqueue");
    let job = claim_job(
        &pool,
        JobType::MetadataExtraction,
        "edge-malformed",
        ChronoDuration::minutes(1),
    )
    .await
    .expect("claim")
    .expect("job");
    let handler = MetadataHandler::new(
        pool.clone(),
        storage.clone(),
        "http://127.0.0.1:9".into(),
        1,
        1,
    );
    // Handler may succeed with generic metadata or fail — either way the file remains.
    let _ = handler.handle(&job).await;
    let _ = complete_job(&pool, job.id).await;

    let node = get_node_by_id(&pool, node_id)
        .await
        .expect("query")
        .expect("node still present");
    assert_eq!(node.lifecycle_state, strife_db::LifecycleState::Active);
    assert!(
        storage
            .exists(StorageKey::original(storage_id))
            .await
            .expect("exists")
    );

    sqlx::query("DELETE FROM file_objects WHERE node_id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .ok();
    let _ = tokio::fs::remove_dir_all(storage_root).await;
}

#[tokio::test]
async fn permanent_delete_is_idempotent_when_storage_already_missing() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL unset; skipping");
        return;
    };
    let storage_root = std::env::temp_dir().join(format!("edge-missing-{}", Uuid::new_v4()));
    let storage = Arc::new(LocalFsBackend::new(&storage_root).await.expect("storage"));
    let node_id = Uuid::new_v4();
    let storage_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("ghost-{node_id}"))
        .execute(&pool)
        .await
        .expect("node");
    sqlx::query(
        r"
        INSERT INTO file_objects (id, node_id, storage_key, byte_size, mime_type, upload_state)
        VALUES ($1, $2, $3, 4, 'application/octet-stream', 'finalized')
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(storage_id.simple().to_string())
    .execute(&pool)
    .await
    .expect("object");
    // Do not create the storage object — it is already "missing".
    trash_node(&pool, node_id).await.expect("trash");
    let deletion = DeletionService::new(pool.clone(), storage);
    let job = enqueue_job(&pool, JobType::PermanentDeletion, node_id, 10)
        .await
        .expect("enqueue")
        .expect("job");
    // Drain until our high-priority job is claimed (queue may contain leftovers).
    let mut leased = None;
    for _ in 0..50 {
        let Some(candidate) = claim_job(
            &pool,
            JobType::PermanentDeletion,
            "edge-ghost",
            ChronoDuration::minutes(1),
        )
        .await
        .expect("claim") else {
            break;
        };
        if candidate.id == job.id || candidate.target_node_id == node_id {
            leased = Some(candidate);
            break;
        }
        // Release unrelated job for later.
        sqlx::query(
            "UPDATE jobs SET state = 'pending', lease_owner = NULL, lease_expires_at = NULL WHERE id = $1",
        )
        .bind(candidate.id)
        .execute(&pool)
        .await
        .ok();
    }
    let leased = leased.expect("our permanent deletion job should be claimable");
    deletion
        .purge(&leased)
        .await
        .expect("purge with missing storage object");
    assert!(
        get_node_by_id(&pool, node_id)
            .await
            .expect("query")
            .is_none()
    );
    let _ = tokio::fs::remove_dir_all(storage_root).await;
}

#[tokio::test]
async fn trash_cleanup_enqueues_batch_of_expired_items() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL unset; skipping");
        return;
    };
    let mut ids = Vec::new();
    for i in 0..20 {
        let folder = strife_db::create_folder(
            &pool,
            ROOT_NODE_ID,
            &format!("edge-purge-{}-{i}", Uuid::new_v4()),
        )
        .await
        .expect("create");
        trash_node(&pool, folder.id).await.expect("trash");
        sqlx::query(
            "UPDATE trash_entries SET scheduled_purge_at = now() - interval '40 days', trashed_at = now() - interval '40 days' WHERE node_id = $1",
        )
        .bind(folder.id)
        .execute(&pool)
        .await
        .expect("expire");
        ids.push(folder.id);
    }
    // Clear prior jobs for these nodes
    sqlx::query("DELETE FROM jobs WHERE target_node_id = ANY($1)")
        .bind(&ids)
        .execute(&pool)
        .await
        .ok();
    let enqueued = enqueue_expired_trash_deletions(&pool, 50)
        .await
        .expect("enqueue expired");
    assert!(
        enqueued >= 20,
        "expected at least 20 enqueued, got {enqueued}"
    );

    let deletion = DeletionService::new(
        pool.clone(),
        Arc::new(
            LocalFsBackend::new(
                std::env::temp_dir().join(format!("edge-purge-{}", Uuid::new_v4())),
            )
            .await
            .expect("storage"),
        ),
    );
    // Purge our fixture nodes directly (job queue may contain unrelated work).
    for id in &ids {
        let job = strife_db::JobRecord {
            id: Uuid::new_v4(),
            job_type: JobType::PermanentDeletion,
            target_node_id: *id,
            state: strife_db::JobState::Leased,
            priority: 0,
            attempts: 1,
            max_attempts: 3,
            lease_owner: Some("edge-cleanup".into()),
            lease_expires_at: None,
            last_error: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        };
        deletion.purge(&job).await.expect("purge expired fixture");
        assert!(
            get_node_by_id(&pool, *id).await.expect("query").is_none(),
            "expired trash node {id} should be purged"
        );
    }
    sqlx::query("DELETE FROM jobs WHERE target_node_id = ANY($1)")
        .bind(&ids)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn exiftool_timeout_is_enforced() {
    // Use a tiny timeout against a path that still invokes the process.
    let path = std::env::temp_dir().join(format!("edge-exif-{}.txt", Uuid::new_v4()));
    tokio::fs::write(&path, b"not image data")
        .await
        .expect("write");
    if Command::new("exiftool").arg("-ver").output().is_err() {
        eprintln!("exiftool unavailable; skipping timeout test");
        let _ = tokio::fs::remove_file(&path).await;
        return;
    }
    let started = Instant::now();
    let result =
        strife_media::extract_exif_with_limits(&path, Duration::from_millis(1), 16 * 1024 * 1024)
            .await;
    let elapsed = started.elapsed();
    assert!(result.is_err(), "expected timeout or process failure");
    assert!(
        elapsed < Duration::from_secs(5),
        "ExifTool call should fail fast under a 1ms timeout, took {elapsed:?}"
    );
    let _ = tokio::fs::remove_file(&path).await;
}

// Keep anyhow::bail available for mock storage trait stubs if extended later.
#[allow(dead_code)]
fn _unused_bail() -> Result<()> {
    bail!("unused")
}
