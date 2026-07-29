use std::sync::Arc;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Duration;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{MIGRATOR, ROOT_NODE_ID};
use strife_storage::LocalFsBackend;
use tower::ServiceExt;
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

async fn send_json(app: axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.oneshot(request).await.expect("send request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response");
    (
        status,
        serde_json::from_slice(&bytes).expect("parse response"),
    )
}

async fn create_folder(app: axum::Router, parent_id: Uuid, name: &str) -> Uuid {
    let (status, body) = send_json(
        app,
        Request::post("/api/folders")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"parent_id": parent_id, "name": name}).to_string(),
            ))
            .expect("build folder request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    Uuid::parse_str(body["id"].as_str().expect("folder id")).expect("valid folder id")
}

async fn child_named(app: axum::Router, parent_id: Uuid, name: &str) -> Value {
    let (status, body) = send_json(
        app,
        Request::get(format!("/api/folders/{parent_id}/children?limit=100"))
            .body(Body::empty())
            .expect("build children request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["name"] == name)
        .cloned()
        .expect("named child")
}

#[tokio::test]
async fn folder_upload_preserves_three_nested_levels() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL API integration test");
        return;
    };
    let root = std::env::temp_dir().join(format!("strife-folder-upload-{}", Uuid::new_v4()));
    let backend = Arc::new(LocalFsBackend::new(&root).await.expect("create storage"));
    let app = strife_api::folders::router(pool.clone()).merge(strife_api::uploads::router(
        pool.clone(),
        backend,
        Duration::hours(24),
        90,
    ));
    let level_one = create_folder(app.clone(), ROOT_NODE_ID, "folder-upload-level-1").await;
    let level_two = create_folder(app.clone(), level_one, "level-2").await;
    let level_three = create_folder(app.clone(), level_two, "level-3").await;

    let (status, session) = send_json(
        app.clone(),
        Request::post("/api/uploads")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"folder_id": level_three, "name": "nested.txt", "size": 6}).to_string(),
            ))
            .expect("build upload request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let session_id =
        Uuid::parse_str(session["session_id"].as_str().expect("session id")).expect("valid id");
    let chunk = app
        .clone()
        .oneshot(
            Request::patch(format!("/api/uploads/{session_id}"))
                .header("content-range", "bytes 0-5/6")
                .body(Body::from("nested"))
                .expect("build chunk request"),
        )
        .await
        .expect("send chunk");
    assert_eq!(chunk.status(), StatusCode::OK);
    let (status, _) = send_json(
        app.clone(),
        Request::post(format!("/api/uploads/{session_id}/finalize"))
            .body(Body::empty())
            .expect("build finalize request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        child_named(app.clone(), ROOT_NODE_ID, "folder-upload-level-1").await["id"],
        level_one.to_string()
    );
    assert_eq!(
        child_named(app.clone(), level_one, "level-2").await["id"],
        level_two.to_string()
    );
    assert_eq!(
        child_named(app.clone(), level_two, "level-3").await["id"],
        level_three.to_string()
    );
    assert_eq!(
        child_named(app, level_three, "nested.txt").await["kind"],
        "file"
    );

    sqlx::query("DELETE FROM upload_sessions WHERE id = $1")
        .bind(session_id)
        .execute(&pool)
        .await
        .expect("remove session");
    sqlx::query(
        "DELETE FROM file_objects WHERE node_id IN (SELECT id FROM nodes WHERE parent_id = $1)",
    )
    .bind(level_three)
    .execute(&pool)
    .await
    .expect("remove object");
    for parent_id in [level_three, level_two, level_one, ROOT_NODE_ID] {
        sqlx::query("DELETE FROM nodes WHERE parent_id = $1 AND name LIKE 'folder-upload-%' OR parent_id = $1 AND name IN ('nested.txt', 'level-2', 'level-3')")
            .bind(parent_id)
            .execute(&pool)
            .await
            .expect("remove hierarchy level");
    }
    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove storage");
}
