//! Full v1 lifecycle: folder → upload → metadata → preview → download → trash → restore → purge.

use std::{process::Command, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Duration;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    ArtifactState, ArtifactType, JobType, MIGRATOR, ROOT_NODE_ID, claim_job, complete_job,
    get_artifact, get_node_by_id, list_trash,
};
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use strife_worker::{JobHandler, MetadataHandler};
use tower::ServiceExt;
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect PostgreSQL");
    MIGRATOR.run(&pool).await.expect("migrate");
    Some(pool)
}

fn app(pool: PgPool, storage: Arc<LocalFsBackend>) -> axum::Router {
    let storage_dyn: Arc<dyn StorageBackend> = storage.clone();
    strife_api::folders::router(pool.clone())
        .merge(strife_api::nodes::router(pool.clone()))
        .merge(strife_api::files::router(pool.clone(), storage_dyn.clone()))
        .merge(strife_api::uploads::router(
            pool,
            storage_dyn,
            Duration::hours(24),
            90,
        ))
}

async fn request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .oneshot(builder.body(body).expect("build request"))
        .await
        .expect("send");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("read body")
        .to_vec();
    (status, bytes)
}

fn generate_jpeg(path: &std::path::Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=size=64x48:color=blue",
            "-frames:v",
            "1",
        ])
        .arg(path)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg must generate JPEG fixture");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn full_lifecycle_folder_upload_metadata_preview_trash_restore_delete() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL unset; skipping e2e lifecycle");
        return;
    };
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("exiftool").arg("-ver").output().is_err()
    {
        eprintln!("ffmpeg/exiftool unavailable; skipping e2e lifecycle");
        return;
    }

    let fixture = Uuid::new_v4();
    let storage_root = std::env::temp_dir().join(format!("strife-e2e-storage-{fixture}"));
    let jpeg_path = std::env::temp_dir().join(format!("strife-e2e-{fixture}.jpg"));
    generate_jpeg(&jpeg_path);
    let content = tokio::fs::read(&jpeg_path).await.expect("read jpeg");
    let storage = Arc::new(
        LocalFsBackend::new(&storage_root)
            .await
            .expect("create storage"),
    );
    let router = app(pool.clone(), storage.clone());

    // 1. Create folder
    let (status, body) = request(
        router.clone(),
        "POST",
        "/api/folders",
        Some(json!({
            "parent_id": ROOT_NODE_ID,
            "name": format!("e2e-{fixture}")
        })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let folder: Value = serde_json::from_slice(&body).expect("folder json");
    let folder_id: Uuid = folder["id"].as_str().unwrap().parse().unwrap();

    // 2. Resumable upload
    let (status, body) = request(
        router.clone(),
        "POST",
        "/api/uploads",
        Some(json!({
            "folder_id": folder_id,
            "name": "lifecycle.jpg",
            "size": content.len(),
            "source_created_at": null,
            "source_modified_at": null
        })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&body).expect("upload json");
    let session_id: Uuid = created["session_id"].as_str().unwrap().parse().unwrap();

    let mid = content.len() / 2;
    let range1 = format!("bytes 0-{}/{len}", mid - 1, len = content.len());
    let response = router
        .clone()
        .oneshot(
            Request::patch(format!("/api/uploads/{session_id}"))
                .header("content-range", &range1)
                .body(Body::from(content[..mid].to_vec()))
                .expect("chunk1"),
        )
        .await
        .expect("send chunk1");
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            Request::patch(format!("/api/uploads/{session_id}"))
                .header(
                    "content-range",
                    format!("bytes {mid}-{}/{len}", content.len() - 1, len = content.len()),
                )
                .body(Body::from(content[mid..].to_vec()))
                .expect("chunk2"),
        )
        .await
        .expect("send chunk2");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .clone()
        .oneshot(
            Request::post(format!("/api/uploads/{session_id}/finalize"))
                .body(Body::empty())
                .expect("finalize"),
        )
        .await
        .expect("send finalize");
    assert_eq!(response.status(), StatusCode::OK);
    let finalized: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body"),
    )
    .expect("finalize json");
    let node_id: Uuid = finalized["id"].as_str().unwrap().parse().unwrap();

    // 3. Metadata extraction via worker handler
    let handler = MetadataHandler::new(
        pool.clone(),
        storage.clone(),
        std::env::var("TIKA_URL").unwrap_or_else(|_| "http://127.0.0.1:9998".into()),
        1,
        2,
    );
    let meta_job = claim_job(
        &pool,
        JobType::MetadataExtraction,
        "e2e-meta",
        Duration::minutes(2),
    )
    .await
    .expect("claim meta")
    .expect("metadata job enqueued on finalize");
    handler.handle(&meta_job).await.expect("metadata");
    complete_job(&pool, meta_job.id)
        .await
        .expect("complete meta");

    let mime: String =
        sqlx::query_scalar("SELECT detected_mime FROM node_metadata WHERE node_id = $1")
            .bind(node_id)
            .fetch_one(&pool)
            .await
            .expect("node metadata");
    assert!(mime.starts_with("image/"), "expected image mime, got {mime}");

    // 4. Preview generation
    let (status, body) = request(
        router.clone(),
        "GET",
        &format!("/api/files/{node_id}/thumbnail"),
        None,
        &[],
    )
    .await;
    assert!(
        status == StatusCode::ACCEPTED || status == StatusCode::OK,
        "thumbnail request status {status}"
    );
    let _ = body;

    if let Some(preview_job) = claim_job(
        &pool,
        JobType::PreviewGeneration,
        "e2e-preview",
        Duration::minutes(2),
    )
    .await
    .expect("claim preview")
    {
        handler.handle(&preview_job).await.expect("preview");
        complete_job(&pool, preview_job.id)
            .await
            .expect("complete preview");
        let artifact = get_artifact(&pool, node_id, ArtifactType::Thumbnail)
            .await
            .expect("get artifact")
            .expect("thumbnail artifact");
        assert_eq!(artifact.state, ArtifactState::Ready);
    }

    // 5. Download byte-for-byte
    let response = router
        .clone()
        .oneshot(
            Request::get(format!("/api/files/{node_id}/download"))
                .body(Body::empty())
                .expect("download"),
        )
        .await
        .expect("send download");
    assert_eq!(response.status(), StatusCode::OK);
    let downloaded = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("download body");
    assert_eq!(downloaded.as_ref(), content.as_slice());

    // Capture original storage key before purge
    let storage_key: String = sqlx::query_scalar(
        "SELECT storage_key FROM file_objects WHERE node_id = $1 AND upload_state = 'finalized'",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("storage key");
    let original_id = Uuid::parse_str(&storage_key).expect("uuid key");

    // 6. Trash
    let (status, _) = request(
        router.clone(),
        "POST",
        &format!("/api/nodes/{node_id}/trash"),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let trash = list_trash(&pool).await.expect("list trash");
    assert!(trash.iter().any(|entry| entry.node_id == node_id));

    // 7. Restore
    let (status, _) = request(
        router.clone(),
        "POST",
        &format!("/api/nodes/{node_id}/restore"),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let restored = get_node_by_id(&pool, node_id)
        .await
        .expect("get")
        .expect("node");
    assert_eq!(
        restored.lifecycle_state,
        strife_db::LifecycleState::Active
    );

    // Trash again then permanent delete
    let (status, _) = request(
        router.clone(),
        "POST",
        &format!("/api/nodes/{node_id}/trash"),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = request(
        router.clone(),
        "DELETE",
        &format!("/api/nodes/{node_id}/permanent"),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let _ = body;

    let deletion = strife_worker::DeletionService::new(pool.clone(), storage.clone());
    let job = claim_job(
        &pool,
        JobType::PermanentDeletion,
        "e2e-delete",
        Duration::minutes(2),
    )
    .await
    .expect("claim delete")
    .expect("deletion job");
    deletion.purge(&job).await.expect("purge");

    assert!(
        get_node_by_id(&pool, node_id)
            .await
            .expect("query")
            .is_none()
    );
    assert!(
        !storage
            .exists(StorageKey::original(original_id))
            .await
            .expect("exists")
    );

    // Cleanup folder
    let _ = request(
        router,
        "POST",
        &format!("/api/nodes/{folder_id}/trash"),
        None,
        &[],
    )
    .await;
    if let Ok(Some(job)) = claim_job(
        &pool,
        JobType::PermanentDeletion,
        "e2e-cleanup",
        Duration::minutes(1),
    )
    .await
    {
        let _ = deletion.purge(&job).await;
    } else {
        let _ = strife_db::request_permanent_deletion(&pool, folder_id).await;
        if let Ok(Some(job)) = claim_job(
            &pool,
            JobType::PermanentDeletion,
            "e2e-cleanup2",
            Duration::minutes(1),
        )
        .await
        {
            let _ = deletion.purge(&job).await;
        }
    }

    let _ = tokio::fs::remove_file(&jpeg_path).await;
    let _ = tokio::fs::remove_dir_all(&storage_root).await;
}

