use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{MIGRATOR, NodeKind, ROOT_NODE_ID, get_node_by_id};
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
async fn migration_creates_the_root_folder() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };

    let root = get_node_by_id(&pool, ROOT_NODE_ID)
        .await
        .expect("query root")
        .expect("root exists");

    assert_eq!(root.parent_id, None);
    assert_eq!(root.name, "root");
    assert_eq!(root.kind, NodeKind::Folder);
}

#[tokio::test]
async fn active_sibling_names_are_unique_and_case_sensitive() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let mut transaction = pool.begin().await.expect("begin transaction");
    let first_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, 'Photos', 'folder')",
    )
    .bind(first_id)
    .bind(ROOT_NODE_ID)
    .execute(&mut *transaction)
    .await
    .expect("insert first sibling");

    let duplicate = sqlx::query(
        "INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, 'Photos', 'folder')",
    )
    .bind(Uuid::new_v4())
    .bind(ROOT_NODE_ID)
    .execute(&mut *transaction)
    .await;
    assert!(duplicate.is_err(), "exact duplicate name must be rejected");

    transaction.rollback().await.expect("rollback test data");

    let mut transaction = pool.begin().await.expect("begin transaction");
    sqlx::query(
        "INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, 'Photos', 'folder'), ($3, $2, 'photos', 'folder')",
    )
    .bind(Uuid::new_v4())
    .bind(ROOT_NODE_ID)
    .bind(Uuid::new_v4())
    .execute(&mut *transaction)
    .await
    .expect("case-distinct siblings are allowed");
    transaction.rollback().await.expect("rollback test data");
}
