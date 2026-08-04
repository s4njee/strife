use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use futures_util::StreamExt;
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    DocumentTextPageInput, DocumentTextSource, DocumentTextStatus, MIGRATOR, ROOT_NODE_ID,
    UpsertDocumentText, append_ocr_event, replace_document_text, set_ocr_engine_state, trash_node,
};
use strife_storage::LocalFsBackend;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url)
        .await
        .expect("connect to integration-test PostgreSQL");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    Some(pool)
}

async fn request(app: Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let body = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("response body");
    (
        status,
        serde_json::from_slice(&body).expect("JSON response"),
    )
}

async fn create_text_file(pool: &PgPool, name: &str, pages: &[&str]) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(name)
        .execute(pool)
        .await
        .expect("create text node");
    let page_inputs = pages
        .iter()
        .enumerate()
        .map(|(index, content)| DocumentTextPageInput {
            page_number: i32::try_from(index + 1).expect("page number"),
            content,
            confidence: Some(if index == 0 { 94.0 } else { 65.0 }),
            width: Some(1200),
            height: Some(1600),
        })
        .collect::<Vec<_>>();
    replace_document_text(
        pool,
        &UpsertDocumentText {
            node_id,
            source: DocumentTextSource::Ocr,
            status: DocumentTextStatus::Completed,
            language: "eng",
            engine_name: "tesseract",
            engine_version: "5.5-api",
            page_count: Some(i32::try_from(pages.len()).expect("page count")),
            mean_confidence: Some(80.0),
            char_count: 100,
            warnings: &["low confidence page".to_owned()],
            duration_ms: Some(100),
        },
        &page_inputs,
    )
    .await
    .expect("store text");
    node_id
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn status_search_text_reprocess_and_sse_contracts() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let mut lock = pool.begin().await.expect("begin OCR API test lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(0x4f43_5200_i64)
        .execute(&mut *lock)
        .await
        .expect("acquire OCR API test lock");
    set_ocr_engine_state(&pool, "tesseract", "5.5-api", "eng")
        .await
        .expect("set engine state");
    let unique_term = format!("apiuniqueterm{}", Uuid::new_v4().simple());
    let first_unique_page = format!("{unique_term} appears here");
    let first = create_text_file(
        &pool,
        &format!("ocr-api-first-{}.pdf", Uuid::new_v4()),
        &["ordinary sibling page", &first_unique_page],
    )
    .await;
    let tree_root = Uuid::new_v4();
    let tree_child = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder'), ($4, $1, $5, 'folder')",
    )
    .bind(tree_root)
    .bind(ROOT_NODE_ID)
    .bind(format!("ocr-tree-root-{tree_root}"))
    .bind(tree_child)
    .bind("Scanned books")
    .execute(&pool)
    .await
    .expect("create OCR tree folders");
    sqlx::query("UPDATE nodes SET parent_id = $2 WHERE id = $1")
        .bind(first)
        .bind(tree_root)
        .execute(&pool)
        .await
        .expect("move first OCR file into tree root");
    sqlx::query(
        "INSERT INTO file_objects (id, node_id, storage_key, byte_size, upload_state) \
         VALUES ($1, $2, $3, 1, 'finalized')",
    )
    .bind(Uuid::new_v4())
    .bind(first)
    .bind(format!("ocr-api/{first}"))
    .execute(&pool)
    .await
    .expect("create reprocessable finalized object");
    let second = create_text_file(
        &pool,
        &format!("ocr-api-second-{}.pdf", Uuid::new_v4()),
        &[&format!("{unique_term} in another document")],
    )
    .await;
    sqlx::query("UPDATE nodes SET parent_id = $2 WHERE id = $1")
        .bind(second)
        .bind(tree_child)
        .execute(&pool)
        .await
        .expect("move second OCR file into nested tree folder");
    let trashed = create_text_file(
        &pool,
        &format!("ocr-api-trash-{}.pdf", Uuid::new_v4()),
        &[&format!("{unique_term} hidden in trash")],
    )
    .await;
    trash_node(&pool, trashed).await.expect("trash search node");
    let storage_root = std::env::temp_dir().join(format!("strife-ocr-api-{}", Uuid::new_v4()));
    let storage = Arc::new(LocalFsBackend::new(&storage_root).await.expect("storage"));
    let app = strife_api::ocr::router(pool.clone())
        .merge(strife_api::search::router(pool.clone()))
        .merge(strife_api::admin::router(pool.clone()))
        .merge(strife_api::files::router(pool.clone(), storage));

    let (status_code, status) = request(app.clone(), "/api/ocr/status").await;
    assert_eq!(status_code, StatusCode::OK);
    assert_eq!(status["engine_version"], "5.5-api");
    assert!(status["counts"]["completed"].as_i64().unwrap_or_default() >= 3);

    let (_, tree_page) = request(
        app.clone(),
        &format!("/api/ocr/tree?parent_id={tree_root}&limit=1"),
    )
    .await;
    assert_eq!(tree_page["items"].as_array().expect("tree items").len(), 1);
    assert_eq!(tree_page["items"][0]["kind"], "folder");
    assert_eq!(tree_page["items"][0]["name"], "Scanned books");
    assert_eq!(tree_page["items"][0]["total_files"], 1);
    assert_eq!(tree_page["items"][0]["completed"], 1);
    assert_eq!(tree_page["next_offset"], 1);
    let (_, second_tree_page) = request(
        app.clone(),
        &format!("/api/ocr/tree?parent_id={tree_root}&limit=1&offset=1"),
    )
    .await;
    assert_eq!(second_tree_page["items"][0]["id"], first.to_string());
    assert_eq!(second_tree_page["items"][0]["status"], "completed");
    assert_eq!(second_tree_page["items"][0]["source"], "ocr");
    assert_eq!(second_tree_page["next_offset"], Value::Null);
    let (_, nested_tree_page) = request(
        app.clone(),
        &format!("/api/ocr/tree?parent_id={tree_child}"),
    )
    .await;
    assert_eq!(nested_tree_page["items"][0]["id"], second.to_string());

    let (empty_code, empty) = request(app.clone(), "/api/search?q=%20%20").await;
    assert_eq!(empty_code, StatusCode::BAD_REQUEST);
    assert_eq!(empty["code"], "bad_request");
    let (_, page_one) = request(app.clone(), &format!("/api/search?q={unique_term}&limit=1")).await;
    assert_eq!(page_one["items"].as_array().expect("search items").len(), 1);
    let cursor = page_one["next_cursor"].as_str().expect("search cursor");
    let (_, page_two) = request(
        app.clone(),
        &format!("/api/search?q={unique_term}&limit=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(
        page_two["items"]
            .as_array()
            .expect("second search page")
            .len(),
        1
    );
    assert_ne!(
        page_one["items"][0]["node_id"],
        page_two["items"][0]["node_id"]
    );
    let (_, including_trash) = request(
        app.clone(),
        &format!("/api/search?q={unique_term}&include_trash=true"),
    )
    .await;
    assert_eq!(
        including_trash["items"]
            .as_array()
            .expect("trash search")
            .len(),
        3
    );

    let (_, text) = request(app.clone(), &format!("/api/files/{first}/text?limit=1")).await;
    assert_eq!(text["status"], "completed");
    assert_eq!(text["pages"].as_array().expect("text pages").len(), 1);
    assert_eq!(text["engine_version"], "5.5-api");
    assert!(text["next_page"].is_number());

    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/api/admin/reprocess?extractor=ocr&scope=node&node_id={first}"
            ))
            .body(Body::empty())
            .expect("reprocess request"),
        )
        .await
        .expect("reprocess response");
    assert_eq!(response.status(), StatusCode::OK);
    let reprocess: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), 16 * 1024)
            .await
            .expect("reprocess response body"),
    )
    .expect("reprocess JSON response");
    assert_eq!(reprocess["extractor"], "ocr");
    assert_eq!(reprocess["enqueued"], 1);

    let event = append_ocr_event(&pool, first, "completed", Some(2), Some(80.0), None)
        .await
        .expect("append SSE fixture");
    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/ocr/events")
                    .header("last-event-id", (event.id - 1).to_string())
                    .body(Body::empty())
                    .expect("SSE request"),
            )
            .await
            .expect("SSE response");
        let mut stream = response.into_body().into_data_stream();
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("SSE event timeout")
            .expect("SSE event")
            .expect("SSE bytes");
        assert!(String::from_utf8_lossy(&chunk).contains("event: entry"));
        drop(stream);
    }
    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("pool remains available after SSE disconnects");
    assert_eq!(one, 1);

    sqlx::query("DELETE FROM file_objects WHERE node_id = $1")
        .bind(first)
        .execute(&pool)
        .await
        .expect("clean up reprocess fixture object");
    for node_id in [first, second, trashed] {
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(node_id)
            .execute(&pool)
            .await
            .expect("clean up OCR API node");
    }
    for folder_id in [tree_child, tree_root] {
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(folder_id)
            .execute(&pool)
            .await
            .expect("clean up OCR tree folder");
    }
    let _ = tokio::fs::remove_dir_all(storage_root).await;
}

/// Rollups must survive more than one level and more than one status.
///
/// The tree query aggregates each folder's own files and then propagates those
/// subtotals up the ancestor chain, rather than carrying one row per file to
/// the root. Anything shallower than a grandparent, or single-status, cannot
/// tell a correct propagation from one that drops or double-counts a level.
async fn folder(pool: &PgPool, parent: Uuid, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder')")
        .bind(id)
        .bind(parent)
        .bind(format!("{name}-{id}"))
        .execute(pool)
        .await
        .expect("create folder");
    id
}

async fn file_with_status(
    pool: &PgPool,
    parent: Uuid,
    name: &str,
    status: DocumentTextStatus,
    source: DocumentTextSource,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(id)
        .bind(parent)
        .bind(format!("{name}-{id}"))
        .execute(pool)
        .await
        .expect("create file");
    replace_document_text(
        pool,
        &UpsertDocumentText {
            node_id: id,
            source,
            status,
            language: "eng",
            engine_name: "tesseract",
            engine_version: "5.5-api",
            page_count: Some(1),
            mean_confidence: Some(80.0),
            char_count: 10,
            warnings: &[],
            duration_ms: Some(1),
        },
        &[],
    )
    .await
    .expect("store text");
    id
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn tree_rollups_accumulate_through_every_ancestor_level() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };

    // root / top / middle / leaf, with files at three different depths so a
    // level that fails to propagate shows up as a specific missing count.
    let top = folder(&pool, ROOT_NODE_ID, "rollup-top").await;
    let middle = folder(&pool, top, "rollup-middle").await;
    let leaf = folder(&pool, middle, "rollup-leaf").await;

    file_with_status(
        &pool,
        top,
        "at-top",
        DocumentTextStatus::Completed,
        DocumentTextSource::Ocr,
    )
    .await;
    file_with_status(
        &pool,
        middle,
        "at-middle",
        DocumentTextStatus::Failed,
        DocumentTextSource::Ocr,
    )
    .await;
    file_with_status(
        &pool,
        leaf,
        "at-leaf-ocr",
        DocumentTextStatus::Completed,
        DocumentTextSource::Ocr,
    )
    .await;
    // `embedded` text reports as skipped rather than completed.
    file_with_status(
        &pool,
        leaf,
        "at-leaf-embedded",
        DocumentTextStatus::Completed,
        DocumentTextSource::Embedded,
    )
    .await;

    let app = strife_api::ocr::router(pool.clone());

    let (_, top_view) = request(app.clone(), &format!("/api/ocr/tree?parent_id={top}")).await;
    let middle_row = top_view["items"]
        .as_array()
        .expect("top items")
        .iter()
        .find(|item| item["id"] == middle.to_string())
        .expect("middle folder listed under top");
    // middle's own failure, plus both of leaf's files.
    assert_eq!(middle_row["total_files"], 3, "{middle_row}");
    assert_eq!(middle_row["completed"], 1, "{middle_row}");
    assert_eq!(middle_row["failed"], 1, "{middle_row}");
    assert_eq!(middle_row["skipped"], 1, "{middle_row}");

    let (_, middle_view) = request(app.clone(), &format!("/api/ocr/tree?parent_id={middle}")).await;
    let leaf_row = middle_view["items"]
        .as_array()
        .expect("middle items")
        .iter()
        .find(|item| item["id"] == leaf.to_string())
        .expect("leaf folder listed under middle");
    assert_eq!(leaf_row["total_files"], 2, "{leaf_row}");
    assert_eq!(leaf_row["completed"], 1, "{leaf_row}");
    assert_eq!(leaf_row["skipped"], 1, "{leaf_row}");
    assert_eq!(leaf_row["failed"], 0, "{leaf_row}");

    let (_, root_view) = request(app, &format!("/api/ocr/tree?parent_id={ROOT_NODE_ID}")).await;
    let top_row = root_view["items"]
        .as_array()
        .expect("root items")
        .iter()
        .find(|item| item["id"] == top.to_string())
        .expect("top folder listed under root");
    // Every file in the subtree, counted once each.
    assert_eq!(top_row["total_files"], 4, "{top_row}");
    assert_eq!(top_row["completed"], 2, "{top_row}");
    assert_eq!(top_row["failed"], 1, "{top_row}");
    assert_eq!(top_row["skipped"], 1, "{top_row}");

    // Deepest first: `nodes_parent_id_fkey` refuses to drop a folder that
    // still has children.
    for id in [leaf, middle, top] {
        sqlx::query("DELETE FROM nodes WHERE parent_id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .expect("clean up folder contents");
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .expect("clean up folder");
    }
}
