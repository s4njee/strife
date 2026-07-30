use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{MIGRATOR, ROOT_NODE_ID, enqueue_reprocessing};
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
async fn reprocessing_is_low_priority_batched_and_idempotent() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let mut nodes = Vec::new();
    for index in 0..12 {
        let node_id = Uuid::new_v4();
        sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
            .bind(node_id)
            .bind(ROOT_NODE_ID)
            .bind(format!("reprocess-{node_id}"))
            .execute(&pool)
            .await
            .expect("create file node");
        sqlx::query(
            "INSERT INTO metadata_records \
             (id, node_id, extractor_name, extractor_version, status) \
             VALUES ($1, $2, 'exiftool', $3, 'completed')",
        )
        .bind(Uuid::new_v4())
        .bind(node_id)
        .bind(if index == 11 { "adapter-v1" } else { "legacy" })
        .execute(&pool)
        .await
        .expect("insert extractor record");
        nodes.push(node_id);
    }

    assert_eq!(
        enqueue_reprocessing(&pool, "exiftool", "adapter-v1")
            .await
            .expect("enqueue old records"),
        10
    );
    assert_eq!(
        enqueue_reprocessing(&pool, "exiftool", "adapter-v1")
            .await
            .expect("repeat enqueue"),
        0
    );
    let priorities: Vec<i32> = sqlx::query_scalar(
        "SELECT priority FROM jobs WHERE target_node_id = ANY($1) ORDER BY priority",
    )
    .bind(&nodes)
    .fetch_all(&pool)
    .await
    .expect("load priorities");
    assert_eq!(priorities, vec![-100; 10]);

    sqlx::query("DELETE FROM nodes WHERE id = ANY($1)")
        .bind(&nodes)
        .execute(&pool)
        .await
        .expect("clean up fixtures");
}
