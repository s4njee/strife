use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    DocumentTextPageInput, DocumentTextSource, DocumentTextStatus, MIGRATOR, ROOT_NODE_ID,
    UpsertDocumentText, count_document_text_by_status, get_document_text, list_document_text_pages,
    replace_document_text_pages, upsert_document_text,
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

fn completed_count(counts: &[strife_db::DocumentTextStatusCount]) -> i64 {
    counts
        .iter()
        .find(|item| item.status == DocumentTextStatus::Completed)
        .map_or(0, |item| item.count)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn document_text_insert_replace_count_and_cascade() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let baseline = completed_count(
        &count_document_text_by_status(&pool)
            .await
            .expect("count baseline document text"),
    );
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("document-text-test-{node_id}"))
        .execute(&pool)
        .await
        .expect("create text fixture node");

    let warnings = vec!["page boundaries supplied by fixture".to_owned()];
    let record = upsert_document_text(
        &pool,
        &UpsertDocumentText {
            node_id,
            source: DocumentTextSource::Ocr,
            status: DocumentTextStatus::Completed,
            language: "eng",
            engine_name: "tesseract",
            engine_version: "5.5.0",
            page_count: Some(2),
            mean_confidence: Some(91.5),
            char_count: 22,
            warnings: &warnings,
            duration_ms: Some(1250),
        },
    )
    .await
    .expect("upsert document text");
    assert_eq!(record.node_id, node_id);
    assert_eq!(record.source, DocumentTextSource::Ocr);
    assert_eq!(record.warnings, warnings);

    replace_document_text_pages(
        &pool,
        node_id,
        &[
            DocumentTextPageInput {
                page_number: 1,
                content: "first page",
                confidence: Some(93.0),
                width: Some(1200),
                height: Some(1600),
            },
            DocumentTextPageInput {
                page_number: 2,
                content: "second page",
                confidence: Some(90.0),
                width: Some(1200),
                height: Some(1600),
            },
        ],
    )
    .await
    .expect("store initial document pages");
    let replaced = replace_document_text_pages(
        &pool,
        node_id,
        &[DocumentTextPageInput {
            page_number: 1,
            content: "replacement",
            confidence: Some(95.0),
            width: Some(1200),
            height: Some(1600),
        }],
    )
    .await
    .expect("replace document pages");
    assert_eq!(replaced.len(), 1);
    let pages = list_document_text_pages(&pool, node_id)
        .await
        .expect("list replaced pages");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].content, "replacement");
    assert!(
        get_document_text(&pool, node_id)
            .await
            .expect("get document text")
            .is_some()
    );
    assert_eq!(
        completed_count(
            &count_document_text_by_status(&pool)
                .await
                .expect("count completed document text"),
        ),
        baseline + 1
    );

    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("delete text fixture node");
    assert!(
        get_document_text(&pool, node_id)
            .await
            .expect("check document cascade")
            .is_none()
    );
    assert!(
        list_document_text_pages(&pool, node_id)
            .await
            .expect("check page cascade")
            .is_empty()
    );
}
