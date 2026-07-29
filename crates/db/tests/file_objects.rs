use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    FileUploadState, MIGRATOR, ROOT_NODE_ID, create_file_object, finalize_file_object,
    get_file_object_by_node_id,
};
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
async fn staged_objects_finalize_once_per_node() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("file-object-test-{node_id}"))
        .execute(&pool)
        .await
        .expect("create file node");

    let staged = create_file_object(
        &pool,
        Uuid::new_v4(),
        12,
        Some("text/plain"),
        Some("abc123"),
    )
    .await
    .expect("create staged object");
    assert_eq!(staged.upload_state, FileUploadState::Staging);
    assert_eq!(staged.node_id, None);

    let finalized = finalize_file_object(&pool, staged.id, node_id)
        .await
        .expect("finalize object");
    assert_eq!(finalized.upload_state, FileUploadState::Finalized);
    assert_eq!(finalized.node_id, Some(node_id));
    assert_eq!(
        get_file_object_by_node_id(&pool, node_id)
            .await
            .expect("load finalized object"),
        Some(finalized)
    );

    let duplicate = create_file_object(&pool, Uuid::new_v4(), 4, None, None)
        .await
        .expect("create second staged object");
    assert!(
        finalize_file_object(&pool, duplicate.id, node_id)
            .await
            .is_err(),
        "a node cannot own two finalized objects"
    );

    sqlx::query("DELETE FROM file_objects WHERE id IN ($1, $2)")
        .bind(staged.id)
        .bind(duplicate.id)
        .execute(&pool)
        .await
        .expect("remove objects");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("remove file node");
}
