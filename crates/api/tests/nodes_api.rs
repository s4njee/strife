use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_api::nodes::{NodeResponse, TrashListResponse};
use strife_db::{MIGRATOR, ROOT_NODE_ID, create_folder, list_children};
use tower::ServiceExt;
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
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

async fn json_request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let builder = Request::builder().method(method).uri(uri);
    let request = if let Some(body) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("build request")
    } else {
        builder.body(Body::empty()).expect("build request")
    };
    app.oneshot(request).await.expect("send request")
}

async fn response_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse response body")
}

#[tokio::test]
async fn trash_restore_and_list_flow() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };

    let parent = create_folder(
        &pool,
        ROOT_NODE_ID,
        &format!("nodes-api-{}", Uuid::new_v4()),
    )
    .await
    .expect("create parent");
    let folder = create_folder(&pool, parent.id, "Reports")
        .await
        .expect("create folder");
    let app = strife_api::nodes::router(pool.clone());

    let trashed = json_request(
        app.clone(),
        "POST",
        &format!("/api/nodes/{}/trash", folder.id),
        None,
    )
    .await;
    assert_eq!(trashed.status(), StatusCode::OK);
    let trashed: NodeResponse = response_json(trashed).await;
    assert_eq!(trashed.id, folder.id);

    let listed = list_children(&pool, parent.id).await.expect("list children");
    assert!(listed.is_empty());

    let trash = json_request(app.clone(), "GET", "/api/trash", None).await;
    assert_eq!(trash.status(), StatusCode::OK);
    let trash: TrashListResponse = response_json(trash).await;
    assert!(trash.items.iter().any(|item| item.node_id == folder.id));

    let restored = json_request(
        app.clone(),
        "POST",
        &format!("/api/nodes/{}/restore", folder.id),
        None,
    )
    .await;
    assert_eq!(restored.status(), StatusCode::OK);
    let restored: NodeResponse = response_json(restored).await;
    assert_eq!(restored.id, folder.id);
    assert_eq!(restored.parent_id, Some(parent.id));

    let listed_again = list_children(&pool, parent.id).await.expect("list again");
    assert_eq!(listed_again.len(), 1);
    assert_eq!(listed_again[0].id, folder.id);

    let batch = create_folder(&pool, parent.id, "A")
        .await
        .expect("create A");
    let batch_b = create_folder(&pool, parent.id, "B")
        .await
        .expect("create B");
    let batch_resp = json_request(
        app,
        "POST",
        "/api/nodes/trash",
        Some(json!({"node_ids": [batch.id, batch_b.id]})),
    )
    .await;
    assert_eq!(batch_resp.status(), StatusCode::OK);

    cleanup_tree(&pool, parent.id).await;
}
