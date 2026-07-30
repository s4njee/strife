use std::{io::Cursor, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{MIGRATOR, ROOT_NODE_ID};
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use tower::ServiceExt;
use uuid::Uuid;

const CONTENT: &[u8] = b"0123456789abcdef";

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

async fn create_file_fixture(pool: &PgPool, storage_key: Uuid) -> (Uuid, Uuid) {
    let folder_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder')")
        .bind(folder_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("download-test-{folder_id}"))
        .execute(pool)
        .await
        .expect("create folder");
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(folder_id)
        .bind("download.bin")
        .execute(pool)
        .await
        .expect("create file node");
    sqlx::query(
        r"
        INSERT INTO file_objects (
            id, node_id, storage_key, byte_size, mime_type, upload_state
        )
        VALUES ($1, $2, $3, $4, 'application/octet-stream', 'finalized')
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(storage_key.simple().to_string())
    .bind(i64::try_from(CONTENT.len()).expect("content length fits"))
    .execute(pool)
    .await
    .expect("create file object");
    (folder_id, node_id)
}

async fn download_request(
    app: axum::Router,
    node_id: Uuid,
    range: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::get(format!("/api/files/{node_id}/download"));
    if let Some(range) = range {
        request = request.header(header::RANGE, range);
    }
    app.oneshot(request.body(Body::empty()).expect("build request"))
        .await
        .expect("send request")
}

async fn json_request(app: axum::Router, uri: String) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::get(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read JSON body");
    (
        status,
        serde_json::from_slice(&body).expect("parse JSON body"),
    )
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn exposes_details_raw_metadata_streams_and_processing_status() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let root = std::env::temp_dir().join(format!("strife-details-api-{}", Uuid::new_v4()));
    let backend = Arc::new(LocalFsBackend::new(&root).await.expect("create storage"));
    let (folder_id, node_id) = create_file_fixture(&pool, Uuid::new_v4()).await;
    sqlx::query(
        r"
        INSERT INTO node_metadata (
            node_id, detected_mime, media_kind, width, height, has_gps,
            gps_latitude, gps_longitude, camera_make, camera_model
        ) VALUES ($1, 'image/jpeg', 'image', 6000, 4000, true, 41.8819, -87.6278, 'NIKON', 'Z 8')
        ",
    )
    .bind(node_id)
    .execute(&pool)
    .await
    .expect("insert normalized metadata");
    for (extractor, status) in [("mime", "completed"), ("exiftool", "failed")] {
        sqlx::query(
            "INSERT INTO metadata_records \
             (id, node_id, extractor_name, extractor_version, status, raw_payload) \
             VALUES ($1, $2, $3, 'adapter-v1', $4::metadata_status, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(node_id)
        .bind(extractor)
        .bind(status)
        .bind(serde_json::json!({"extractor": extractor, "retained": true}))
        .execute(&pool)
        .await
        .expect("insert metadata record");
    }
    sqlx::query(
        "INSERT INTO media_streams \
         (id, node_id, stream_index, stream_type, codec) VALUES ($1, $2, 0, 'video', 'mjpeg')",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .execute(&pool)
    .await
    .expect("insert media stream");
    let app = strife_api::files::router(pool.clone(), backend);

    let (status, details) = json_request(app.clone(), format!("/api/files/{node_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(details["processing_status"], "partially_processed");
    assert_eq!(details["detected_mime"], "image/jpeg");
    assert_eq!(details["gps_latitude"], 41.8819);
    assert_eq!(details["gps_longitude"], -87.6278);

    let (_, metadata) = json_request(app.clone(), format!("/api/files/{node_id}/metadata")).await;
    assert_eq!(metadata.as_array().expect("metadata array").len(), 2);
    assert!(metadata[0]["raw_payload"].is_null());
    let (_, raw_metadata) = json_request(
        app.clone(),
        format!("/api/files/{node_id}/metadata?raw=true"),
    )
    .await;
    assert!(raw_metadata[0]["raw_payload"].is_object());

    let (_, streams) = json_request(app, format!("/api/files/{node_id}/streams")).await;
    assert_eq!(streams[0]["codec"], "mjpeg");

    // file_objects reference nodes with ON DELETE RESTRICT.
    sqlx::query("DELETE FROM file_objects WHERE node_id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("delete file object fixture");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("delete file fixture");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .expect("delete folder fixture");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn downloads_stream_full_and_partial_original_bytes() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let root = std::env::temp_dir().join(format!("strife-download-api-{}", Uuid::new_v4()));
    let backend = Arc::new(LocalFsBackend::new(&root).await.expect("create storage"));
    let storage_key = Uuid::new_v4();
    backend
        .put_stream(
            StorageKey::original(storage_key),
            Box::pin(Cursor::new(CONTENT)),
        )
        .await
        .expect("store original");
    let (folder_id, node_id) = create_file_fixture(&pool, storage_key).await;
    let app = strife_api::files::router(pool.clone(), backend);

    let full = download_request(app.clone(), node_id, None).await;
    assert_eq!(full.status(), StatusCode::OK);
    assert_eq!(full.headers()[header::CONTENT_LENGTH], "16");
    assert_eq!(
        full.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert_eq!(
        full.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"download.bin\""
    );
    assert_eq!(
        to_bytes(full.into_body(), 1024).await.expect("read body"),
        CONTENT
    );

    let partial = download_request(app.clone(), node_id, Some("bytes=4-9")).await;
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(partial.headers()[header::CONTENT_LENGTH], "6");
    assert_eq!(partial.headers()[header::CONTENT_RANGE], "bytes 4-9/16");
    assert_eq!(
        to_bytes(partial.into_body(), 1024)
            .await
            .expect("read partial body"),
        &CONTENT[4..=9]
    );

    assert_eq!(
        download_request(app.clone(), Uuid::new_v4(), None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE nodes SET lifecycle_state = 'trashed' WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("trash file");
    assert_eq!(
        download_request(app, node_id, None).await.status(),
        StatusCode::NOT_FOUND
    );

    sqlx::query("DELETE FROM file_objects WHERE node_id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("remove file object");
    sqlx::query("DELETE FROM nodes WHERE id IN ($1, $2)")
        .bind(node_id)
        .bind(folder_id)
        .execute(&pool)
        .await
        .expect("remove fixtures");
    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove storage");
}
