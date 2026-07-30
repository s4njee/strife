use std::sync::Arc;

use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    ArtifactState, ArtifactType, JobType, MIGRATOR, ROOT_NODE_ID, UpsertArtifact,
    create_file_object, create_folder, create_or_update_artifact, finalize_file_object,
    get_node_by_id, request_permanent_deletion, trash_node,
};
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use strife_worker::{DeletionService, JobHandler, WorkerHandler};
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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn permanent_deletion_removes_storage_and_db_rows() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };

    let storage_root =
        std::env::temp_dir().join(format!("strife-deletion-storage-{}", Uuid::new_v4()));
    let storage = Arc::new(
        LocalFsBackend::new(&storage_root)
            .await
            .expect("create storage"),
    );

    let parent = create_folder(
        &pool,
        ROOT_NODE_ID,
        &format!("del-parent-{}", Uuid::new_v4()),
    )
    .await
    .expect("create parent");

    let original_id = Uuid::new_v4();
    let artifact_id = Uuid::new_v4();
    storage
        .put_stream(
            StorageKey::original(original_id),
            Box::pin(std::io::Cursor::new(b"hello-original".to_vec())),
        )
        .await
        .expect("store original");
    storage
        .put_stream(
            StorageKey::artifact(artifact_id),
            Box::pin(std::io::Cursor::new(b"thumb".to_vec())),
        )
        .await
        .expect("store artifact");

    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(parent.id)
        .bind("doomed.txt")
        .execute(&pool)
        .await
        .expect("create file node");

    let file_object = create_file_object(&pool, original_id, 14, None, None)
        .await
        .expect("create file object");
    finalize_file_object(&pool, file_object.id, node_id)
        .await
        .expect("finalize file object");

    let artifact_key = artifact_id.simple().to_string();
    create_or_update_artifact(
        &pool,
        &UpsertArtifact {
            node_id,
            artifact_type: ArtifactType::Thumbnail,
            format: "webp",
            width: Some(32),
            height: Some(32),
            storage_key: &artifact_key,
            byte_size: 5,
            generator_version: "test-v1",
            state: ArtifactState::Ready,
        },
    )
    .await
    .expect("create artifact row");

    trash_node(&pool, node_id).await.expect("trash file");
    let job = request_permanent_deletion(&pool, node_id)
        .await
        .expect("enqueue deletion")
        .expect("job created");

    let handler = DeletionService::new(pool.clone(), storage.clone());
    // Use the enqueued job record directly; shared CI queues can starve claim_job.
    handler.purge(&job).await.expect("run permanent deletion");

    assert!(
        get_node_by_id(&pool, node_id)
            .await
            .expect("query node")
            .is_none(),
        "node row must be deleted"
    );
    assert!(
        !storage
            .exists(StorageKey::original(original_id))
            .await
            .expect("exists original"),
        "original must be removed from storage"
    );
    assert!(
        !storage
            .exists(StorageKey::artifact(artifact_id))
            .await
            .expect("exists artifact"),
        "artifact must be removed from storage"
    );

    // Idempotent re-run.
    let missing_job = strife_db::JobRecord {
        id: Uuid::new_v4(),
        job_type: JobType::PermanentDeletion,
        target_node_id: node_id,
        state: strife_db::JobState::Leased,
        priority: 0,
        attempts: 1,
        max_attempts: 3,
        lease_owner: Some("test".into()),
        lease_expires_at: None,
        last_error: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: None,
    };
    handler
        .purge(&missing_job)
        .await
        .expect("idempotent purge of missing node");

    // Cleanup parent folder (still active).
    trash_node(&pool, parent.id).await.expect("trash parent");
    if let Ok(Some(parent_job)) = request_permanent_deletion(&pool, parent.id).await {
        handler.purge(&parent_job).await.expect("purge parent");
    }

    let _ = tokio::fs::remove_dir_all(&storage_root).await;
}

#[tokio::test]
async fn worker_handler_routes_permanent_deletion() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let storage_root = std::env::temp_dir().join(format!("strife-deletion-wh-{}", Uuid::new_v4()));
    let storage = Arc::new(
        LocalFsBackend::new(&storage_root)
            .await
            .expect("create storage"),
    );

    let folder = create_folder(
        &pool,
        ROOT_NODE_ID,
        &format!("del-folder-{}", Uuid::new_v4()),
    )
    .await
    .expect("create folder");
    trash_node(&pool, folder.id).await.expect("trash");
    let job = request_permanent_deletion(&pool, folder.id)
        .await
        .expect("enqueue")
        .expect("job");

    let handler = WorkerHandler::new(pool.clone(), storage, "http://127.0.0.1:9".into(), 1, 1);
    handler.handle(&job).await.expect("handle deletion");

    assert!(
        get_node_by_id(&pool, folder.id)
            .await
            .expect("query")
            .is_none()
    );

    let _ = tokio::fs::remove_dir_all(storage_root).await;
}
