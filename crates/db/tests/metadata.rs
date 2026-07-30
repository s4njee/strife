use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{MIGRATOR, MediaStreamInput, MediaStreamType, ROOT_NODE_ID, replace_media_streams};
use uuid::Uuid;

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

#[tokio::test]
async fn metadata_schema_persists_raw_typed_and_stream_data() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("metadata-test-{node_id}"))
        .execute(&pool)
        .await
        .expect("create file node");

    sqlx::query(
        "INSERT INTO metadata_records \
         (id, node_id, extractor_name, extractor_version, status, raw_payload, warnings) \
         VALUES ($1, $2, 'ffprobe', '7.1', 'completed', $3, ARRAY['sample warning'])",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(serde_json::json!({"format": {"duration": "1.25"}}))
    .execute(&pool)
    .await
    .expect("insert raw metadata");

    let duplicate = sqlx::query(
        "INSERT INTO metadata_records \
         (id, node_id, extractor_name, extractor_version) VALUES ($1, $2, 'ffprobe', '7.2')",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .execute(&pool)
    .await;
    assert!(
        duplicate.is_err(),
        "extractor records must be unique per node"
    );

    sqlx::query(
        "INSERT INTO node_metadata \
         (node_id, detected_mime, media_kind, duration_ms, width, height, has_gps) \
         VALUES ($1, 'video/mp4', 'video', 1250, 1920, 1080, false)",
    )
    .bind(node_id)
    .execute(&pool)
    .await
    .expect("insert normalized metadata");
    replace_media_streams(
        &pool,
        node_id,
        &[MediaStreamInput {
            stream_index: 0,
            stream_type: MediaStreamType::Video,
            codec: "h264",
            width: Some(1920),
            height: Some(1080),
            duration_ms: Some(1250),
            bitrate_bps: Some(2_000_000),
            frame_rate: Some("30/1"),
            language: None,
        }],
    )
    .await
    .expect("replace media streams");

    let raw: serde_json::Value = sqlx::query_scalar(
        "SELECT raw_payload FROM metadata_records WHERE node_id = $1 AND extractor_name = 'ffprobe'",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("load raw metadata");
    assert_eq!(raw["format"]["duration"], "1.25");

    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("clean up node metadata cascade");
}
