use std::{fs, process::Command, sync::Arc};

use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    ArtifactState, ArtifactType, JobState, JobType, MIGRATOR, ROOT_NODE_ID, UpsertArtifact,
    create_or_update_artifact, enqueue_job, get_artifact,
};
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use strife_worker::{JobHandler, MetadataHandler};
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
async fn jpeg_job_persists_raw_and_normalized_metadata() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("exiftool").arg("-ver").output().is_err()
    {
        eprintln!("ffmpeg or ExifTool unavailable; skipping extractor integration test");
        return;
    }

    let fixture = std::env::temp_dir().join(format!("strife-worker-{}.jpg", Uuid::new_v4()));
    let storage_root =
        std::env::temp_dir().join(format!("strife-worker-storage-{}", Uuid::new_v4()));
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
        .arg(&fixture)
        .status()
        .expect("generate JPEG fixture");
    assert!(status.success());

    let storage = Arc::new(
        LocalFsBackend::new(&storage_root)
            .await
            .expect("create storage"),
    );
    let storage_id = Uuid::new_v4();
    storage
        .put_stream(
            StorageKey::original(storage_id),
            Box::pin(tokio::fs::File::open(&fixture).await.expect("open fixture")),
        )
        .await
        .expect("store fixture");
    let byte_size = i64::try_from(fs::metadata(&fixture).expect("fixture metadata").len())
        .expect("fixture size fits i64");
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("metadata-job-{node_id}.jpg"))
        .execute(&pool)
        .await
        .expect("create file node");
    sqlx::query(
        r"
        INSERT INTO file_objects (
            id, node_id, storage_key, byte_size, mime_type, checksum_sha256, upload_state
        ) VALUES ($1, $2, $3, $4, 'application/octet-stream', 'fixture-checksum', 'finalized')
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(storage_id.simple().to_string())
    .bind(byte_size)
    .execute(&pool)
    .await
    .expect("create finalized file object");

    let job = enqueue_job(&pool, JobType::MetadataExtraction, node_id, 0)
        .await
        .expect("enqueue metadata job")
        .expect("metadata job");
    let handler = MetadataHandler::new(
        pool.clone(),
        storage.clone(),
        "http://127.0.0.1:9998".to_owned(),
        1,
        2,
    );
    handler.handle(&job).await.expect("process JPEG metadata");
    // Mark completed without leasing — shared CI queues may have unrelated work.
    sqlx::query(
        "UPDATE jobs SET state = 'completed', completed_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(job.id)
    .execute(&pool)
    .await
    .expect("complete metadata job");
    let completed = strife_db::get_job(&pool, job.id)
        .await
        .expect("load completed job")
        .expect("job exists");
    assert_eq!(completed.state, JobState::Completed);

    let records: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM metadata_records WHERE node_id = $1 AND status = 'completed'",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("count metadata records");
    assert_eq!(records, 2);
    let normalized: (String, i32, i32) =
        sqlx::query_as("SELECT detected_mime, width, height FROM node_metadata WHERE node_id = $1")
            .bind(node_id)
            .fetch_one(&pool)
            .await
            .expect("load normalized metadata");
    assert_eq!(normalized, ("image/jpeg".to_owned(), 64, 48));
    let raw_is_array: bool = sqlx::query_scalar(
        "SELECT jsonb_typeof(raw_payload) = 'array' FROM metadata_records \
         WHERE node_id = $1 AND extractor_name = 'exiftool'",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("check raw ExifTool payload");
    assert!(raw_is_array);

    let artifact_id = Uuid::new_v4();
    let artifact_key = artifact_id.simple().to_string();
    create_or_update_artifact(
        &pool,
        &UpsertArtifact {
            node_id,
            artifact_type: ArtifactType::Thumbnail,
            format: "application/octet-stream",
            width: None,
            height: None,
            storage_key: &artifact_key,
            byte_size: 0,
            generator_version: "preview-v1",
            state: ArtifactState::Generating,
        },
    )
    .await
    .expect("create generating artifact");
    let preview_job = enqueue_job(&pool, JobType::PreviewGeneration, node_id, 0)
        .await
        .expect("enqueue preview job")
        .expect("preview job");
    handler
        .handle(&preview_job)
        .await
        .expect("generate preview");
    sqlx::query(
        "UPDATE jobs SET state = 'completed', completed_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(preview_job.id)
    .execute(&pool)
    .await
    .expect("complete preview job");
    let artifact = get_artifact(&pool, node_id, ArtifactType::Thumbnail)
        .await
        .expect("load artifact")
        .expect("artifact exists");
    assert_eq!(artifact.state, ArtifactState::Ready);
    // Small fixtures may keep native dimensions when already under the 256px target.
    assert!(artifact.width.unwrap_or(0) > 0 && artifact.height.unwrap_or(0) > 0);
    assert!(
        storage
            .exists(StorageKey::artifact(artifact_id))
            .await
            .expect("check artifact storage")
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
        .expect("clean up database fixture");
    let _ = fs::remove_file(fixture);
    let _ = fs::remove_dir_all(storage_root);
}
