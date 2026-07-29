use chrono::Utc;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    DEFAULT_IMPORT_SOURCE_ID, ImportEntryState, MIGRATOR, ROOT_NODE_ID, UpsertImportEntry,
    get_import_source, list_pending_entries, mark_import_failed, mark_imported,
    upsert_import_entry,
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
async fn fixed_source_and_entry_lifecycle_are_persisted() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let source = get_import_source(&pool, DEFAULT_IMPORT_SOURCE_ID)
        .await
        .expect("load source")
        .expect("fixed source exists");
    assert_eq!(source.watch_path, "/mnt/ext/watch");
    assert_eq!(source.destination_folder_id, ROOT_NODE_ID);

    let unique_path = format!("db-test-{}/photo.jpg", Uuid::new_v4());
    let first = upsert_import_entry(
        &pool,
        UpsertImportEntry {
            source_id: source.id,
            source_path: &unique_path,
            source_size: 42,
            source_modified_at: Utc::now(),
        },
    )
    .await
    .expect("discover entry");
    let second = upsert_import_entry(
        &pool,
        UpsertImportEntry {
            source_id: source.id,
            source_path: &unique_path,
            source_size: 42,
            source_modified_at: first.source_modified_at,
        },
    )
    .await
    .expect("repeat discovery");
    assert_eq!(first.id, second.id);
    assert_eq!(second.state, ImportEntryState::Discovered);
    assert!(
        list_pending_entries(&pool, source.id)
            .await
            .expect("pending entries")
            .iter()
            .any(|entry| entry.id == first.id)
    );

    mark_import_failed(&pool, first.id, "destination name conflict")
        .await
        .expect("mark failed");
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("import-entry-test-{node_id}"))
        .execute(&pool)
        .await
        .expect("create result node");
    let imported = mark_imported(&pool, first.id, node_id, "abc123")
        .await
        .expect("mark imported");
    assert_eq!(imported.state, ImportEntryState::Imported);
    assert_eq!(imported.resulting_node_id, Some(node_id));
    assert_eq!(imported.source_checksum.as_deref(), Some("abc123"));

    sqlx::query("DELETE FROM import_entries WHERE id = $1")
        .bind(first.id)
        .execute(&pool)
        .await
        .expect("remove entry");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("remove node");
}
