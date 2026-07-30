use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    MIGRATOR, ROOT_NODE_ID, add_favorite, create_folder, list_favorites, remove_favorite,
    trash_node,
};
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    Some(pool)
}

#[tokio::test]
async fn favorites_are_idempotent_and_removed_on_trash() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping");
        return;
    };

    let folder = create_folder(
        &pool,
        ROOT_NODE_ID,
        &format!("fav-{}", Uuid::new_v4()),
    )
    .await
    .expect("create");

    add_favorite(&pool, folder.id).await.expect("favorite");
    add_favorite(&pool, folder.id)
        .await
        .expect("idempotent favorite");

    let listed = list_favorites(&pool).await.expect("list");
    assert_eq!(
        listed.iter().filter(|item| item.node_id == folder.id).count(),
        1
    );

    remove_favorite(&pool, folder.id).await.expect("unfavorite");
    add_favorite(&pool, folder.id).await.expect("re-favorite");

    trash_node(&pool, folder.id).await.expect("trash");
    let after_trash = list_favorites(&pool).await.expect("list after trash");
    assert!(after_trash.iter().all(|item| item.node_id != folder.id));
}
