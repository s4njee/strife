use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    LifecycleState, MIGRATOR, NodeKind, ROOT_NODE_ID, create_folder, get_node_by_id, list_children,
    list_trash, restore_node, trash_node,
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

async fn cleanup_tree(pool: &PgPool, root_id: Uuid) {
    sqlx::query(
        r"
        WITH RECURSIVE tree AS (
            SELECT id FROM nodes WHERE id = $1
            UNION ALL
            SELECT child.id
            FROM nodes AS child
            JOIN tree AS parent ON child.parent_id = parent.id
        )
        DELETE FROM nodes WHERE id IN (SELECT id FROM tree)
        ",
    )
    .bind(root_id)
    .execute(pool)
    .await
    .expect("cleanup fixture tree");
}

#[tokio::test]
async fn trash_excludes_from_listing_and_restore_reincludes() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };

    let parent = create_folder(&pool, ROOT_NODE_ID, &format!("trash-parent-{}", Uuid::new_v4()))
        .await
        .expect("create parent");
    let folder = create_folder(&pool, parent.id, "Projects")
        .await
        .expect("create folder");
    let child = create_folder(&pool, folder.id, "Nested")
        .await
        .expect("create nested folder");

    let children_before = list_children(&pool, parent.id).await.expect("list before");
    assert_eq!(children_before.len(), 1);
    assert_eq!(children_before[0].id, folder.id);

    let trashed = trash_node(&pool, folder.id).await.expect("trash folder");
    assert_eq!(trashed.lifecycle_state, LifecycleState::Trashed);

    let children_after = list_children(&pool, parent.id).await.expect("list after trash");
    assert!(children_after.is_empty(), "trashed folder must leave active listing");

    let nested = get_node_by_id(&pool, child.id)
        .await
        .expect("query nested")
        .expect("nested exists");
    assert_eq!(nested.lifecycle_state, LifecycleState::Trashed);

    let trash_list = list_trash(&pool).await.expect("list trash");
    assert!(
        trash_list.iter().any(|entry| entry.node_id == folder.id),
        "trash listing includes top-level item"
    );
    assert!(
        trash_list.iter().all(|entry| entry.node_id != child.id),
        "nested descendants without their own trash entry are not listed"
    );

    let restored = restore_node(&pool, folder.id).await.expect("restore folder");
    assert_eq!(restored.lifecycle_state, LifecycleState::Active);
    assert_eq!(restored.parent_id, Some(parent.id));

    let children_restored = list_children(&pool, parent.id).await.expect("list after restore");
    assert_eq!(children_restored.len(), 1);
    assert_eq!(children_restored[0].id, folder.id);

    let nested_restored = get_node_by_id(&pool, child.id)
        .await
        .expect("query nested restored")
        .expect("nested exists");
    assert_eq!(nested_restored.lifecycle_state, LifecycleState::Active);
    assert_eq!(nested_restored.kind, NodeKind::Folder);

    let trash_after = list_trash(&pool).await.expect("list trash after restore");
    assert!(trash_after.iter().all(|entry| entry.node_id != folder.id));

    cleanup_tree(&pool, parent.id).await;
}

#[tokio::test]
async fn restore_falls_back_to_root_when_parent_is_not_active() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };

    let parent = create_folder(
        &pool,
        ROOT_NODE_ID,
        &format!("trash-gone-parent-{}", Uuid::new_v4()),
    )
    .await
    .expect("create parent");
    let folder = create_folder(&pool, parent.id, "OrphanMe")
        .await
        .expect("create folder");

    // Trash the child first so it owns its own trash entry with original parent.
    trash_node(&pool, folder.id).await.expect("trash folder");
    // Then trash the parent so the restore destination is no longer active.
    trash_node(&pool, parent.id).await.expect("trash parent");

    let restored = restore_node(&pool, folder.id)
        .await
        .expect("restore orphaned folder");
    assert_eq!(restored.parent_id, Some(ROOT_NODE_ID));
    assert_eq!(restored.lifecycle_state, LifecycleState::Active);

    cleanup_tree(&pool, parent.id).await;
    cleanup_tree(&pool, folder.id).await;
}

#[tokio::test]
async fn cannot_trash_root() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };

    let error = trash_node(&pool, ROOT_NODE_ID)
        .await
        .expect_err("root must be rejected");
    assert!(matches!(
        error,
        strife_db::TrashMutationError::CannotTrashRoot
    ));
}
