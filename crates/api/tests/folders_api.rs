use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_api::folders::{AncestorResponse, ChildrenResponse, FolderResponse};
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

#[tokio::test]
async fn ancestors_are_ordered_from_root_to_current_folder() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let fixture_id = create_fixture_parent(&pool).await;
    let child = strife_db::create_folder(&pool, fixture_id, "Child")
        .await
        .expect("create child");
    let grandchild = strife_db::create_folder(&pool, child.id, "Grandchild")
        .await
        .expect("create grandchild");
    let app = strife_api::folders::router(pool.clone());

    let response = app
        .oneshot(
            Request::get(format!("/api/folders/{}/ancestors", grandchild.id))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("list ancestors");
    assert_eq!(response.status(), StatusCode::OK);
    let ancestors: Vec<AncestorResponse> = response_json(response).await;

    assert_eq!(ancestors.first().map(|item| item.id), Some(ROOT_NODE_ID));
    assert_eq!(
        ancestors
            .iter()
            .rev()
            .take(2)
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["Grandchild", "Child"]
    );

    remove_fixture_tree(&pool, fixture_id).await;
}

#[tokio::test]
async fn batch_move_is_atomic_and_reports_conflicting_folders() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let fixture_id = create_fixture_parent(&pool).await;
    let source = strife_db::create_folder(&pool, fixture_id, "Source")
        .await
        .expect("create source");
    let alpha = strife_db::create_folder(&pool, source.id, "Alpha")
        .await
        .expect("create alpha");
    let bravo = strife_db::create_folder(&pool, source.id, "Bravo")
        .await
        .expect("create bravo");
    let conflicted_destination = strife_db::create_folder(&pool, fixture_id, "Conflicted")
        .await
        .expect("create conflicted destination");
    strife_db::create_folder(&pool, conflicted_destination.id, "Alpha")
        .await
        .expect("create conflicting child");
    let app = strife_api::folders::router(pool.clone());

    let conflict = json_request(
        app.clone(),
        "PATCH",
        "/api/folders/move",
        json!({
            "folder_ids": [alpha.id, bravo.id],
            "parent_id": conflicted_destination.id
        }),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    let conflict: Value = response_json(conflict).await;
    assert_eq!(conflict["code"], "move_conflict");
    assert_eq!(conflict["conflicts"][0]["id"], alpha.id.to_string());
    assert_eq!(conflict["conflicts"][0]["reason"], "name_conflict");
    assert_eq!(
        strife_db::get_node_by_id(&pool, alpha.id)
            .await
            .expect("load alpha")
            .expect("alpha exists")
            .parent_id,
        Some(source.id)
    );
    assert_eq!(
        strife_db::get_node_by_id(&pool, bravo.id)
            .await
            .expect("load bravo")
            .expect("bravo exists")
            .parent_id,
        Some(source.id)
    );

    let destination = strife_db::create_folder(&pool, fixture_id, "Destination")
        .await
        .expect("create destination");
    let moved = json_request(
        app,
        "PATCH",
        "/api/folders/move",
        json!({
            "folder_ids": [alpha.id, bravo.id],
            "parent_id": destination.id
        }),
    )
    .await;
    assert_eq!(moved.status(), StatusCode::OK);
    let moved: Value = response_json(moved).await;
    assert_eq!(moved["items"].as_array().map(Vec::len), Some(2));
    for folder_id in [alpha.id, bravo.id] {
        assert_eq!(
            strife_db::get_node_by_id(&pool, folder_id)
                .await
                .expect("load moved folder")
                .expect("moved folder exists")
                .parent_id,
            Some(destination.id)
        );
    }

    remove_fixture_tree(&pool, fixture_id).await;
}
