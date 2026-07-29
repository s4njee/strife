use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_api::folders::{ChildrenResponse, FolderResponse};
use strife_db::{MIGRATOR, ROOT_NODE_ID};
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

async fn create_fixture_parent(pool: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder')")
        .bind(id)
        .bind(ROOT_NODE_ID)
        .bind(format!("api-test-{id}"))
        .execute(pool)
        .await
        .expect("create fixture parent");
    id
}

async fn remove_fixture_tree(pool: &PgPool, root_id: Uuid) {
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
    .expect("remove fixture tree");
}

async fn json_request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Value,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("build request"),
    )
    .await
    .expect("send request")
}

async fn response_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse response body")
}

#[tokio::test]
async fn create_rename_move_conflict_and_cycle_rejection() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let fixture_id = create_fixture_parent(&pool).await;
    let app = strife_api::folders::router(pool.clone());

    let created = json_request(
        app.clone(),
        "POST",
        "/api/folders",
        json!({"parent_id": fixture_id, "name": "Projects"}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: FolderResponse = response_json(created).await;

    let conflict = json_request(
        app.clone(),
        "POST",
        "/api/folders",
        json!({"parent_id": fixture_id, "name": "Projects"}),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let renamed = json_request(
        app.clone(),
        "PATCH",
        &format!("/api/folders/{}", created.id),
        json!({"name": "Work"}),
    )
    .await;
    assert_eq!(renamed.status(), StatusCode::OK);
    let renamed: FolderResponse = response_json(renamed).await;
    assert_eq!(renamed.name, "Work");

    let destination = json_request(
        app.clone(),
        "POST",
        "/api/folders",
        json!({"parent_id": fixture_id, "name": "Archive"}),
    )
    .await;
    let destination: FolderResponse = response_json(destination).await;

    let moved = json_request(
        app.clone(),
        "PATCH",
        &format!("/api/folders/{}", renamed.id),
        json!({"parent_id": destination.id}),
    )
    .await;
    assert_eq!(moved.status(), StatusCode::OK);

    let cycle = json_request(
        app,
        "PATCH",
        &format!("/api/folders/{}", destination.id),
        json!({"parent_id": renamed.id}),
    )
    .await;
    assert_eq!(cycle.status(), StatusCode::BAD_REQUEST);

    remove_fixture_tree(&pool, fixture_id).await;
}

#[tokio::test]
async fn children_are_name_sorted_and_cursor_paginated() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let fixture_id = create_fixture_parent(&pool).await;
    for name in ["Charlie", "Alpha", "Bravo"] {
        strife_db::create_folder(&pool, fixture_id, name)
            .await
            .expect("create child");
    }
    let app = strife_api::folders::router(pool.clone());

    let first = app
        .clone()
        .oneshot(
            Request::get(format!("/api/folders/{fixture_id}/children?limit=2"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("list first page");
    assert_eq!(first.status(), StatusCode::OK);
    let first: ChildrenResponse = response_json(first).await;
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "Bravo"]
    );
    let cursor = first.next_cursor.expect("first page has a cursor");

    let second = app
        .oneshot(
            Request::get(format!(
                "/api/folders/{fixture_id}/children?limit=2&cursor={cursor}"
            ))
            .body(Body::empty())
            .expect("build request"),
        )
        .await
        .expect("list second page");
    let second: ChildrenResponse = response_json(second).await;
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].name, "Charlie");
    assert_eq!(second.next_cursor, None);

    remove_fixture_tree(&pool, fixture_id).await;
}
