use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    ArtifactState, ArtifactType, MIGRATOR, ROOT_NODE_ID, UpsertArtifact, create_or_update_artifact,
    get_artifact,
};
use uuid::Uuid;

#[tokio::test]
async fn artifacts_upsert_once_per_node_and_type() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let pool: PgPool = PgPoolOptions::new().connect(&url).await.unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    let node = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes(id,parent_id,name,kind) VALUES($1,$2,$3,'file')")
        .bind(node)
        .bind(ROOT_NODE_ID)
        .bind(format!("artifact-{node}"))
        .execute(&pool)
        .await
        .unwrap();
    for state in [ArtifactState::Generating, ArtifactState::Ready] {
        create_or_update_artifact(
            &pool,
            &UpsertArtifact {
                node_id: node,
                artifact_type: ArtifactType::Thumbnail,
                format: "image/webp",
                width: Some(256),
                height: Some(128),
                storage_key: "abc",
                byte_size: 42,
                generator_version: "v1",
                state,
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(
        get_artifact(&pool, node, ArtifactType::Thumbnail)
            .await
            .unwrap()
            .unwrap()
            .state,
        ArtifactState::Ready
    );
    sqlx::query("DELETE FROM nodes WHERE id=$1")
        .bind(node)
        .execute(&pool)
        .await
        .unwrap();
}
