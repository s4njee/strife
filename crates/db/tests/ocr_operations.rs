use chrono::Duration;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    DocumentTextPageInput, DocumentTextSource, DocumentTextStatus, JobType, MIGRATOR,
    OcrReprocessScope, ROOT_NODE_ID, UpsertDocumentText, append_ocr_event, claim_job,
    create_file_object, enqueue_job, enqueue_ocr_reprocessing, finalize_file_object,
    get_ocr_engine_state, get_ocr_status_counts, list_ocr_events_after, replace_document_text,
    search_document_text, set_ocr_engine_state, trash_node,
};
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("connect to integration-test PostgreSQL");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    Some(pool)
}

async fn create_file(pool: &PgPool, name: &str) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(name)
        .execute(pool)
        .await
        .expect("create OCR file");
    let object = create_file_object(pool, Uuid::new_v4(), 10, Some("image/png"), None)
        .await
        .expect("create OCR object");
    finalize_file_object(pool, object.id, node_id)
        .await
        .expect("finalize OCR object");
    node_id
}

async fn store_text(
    pool: &PgPool,
    node_id: Uuid,
    source: DocumentTextSource,
    status: DocumentTextStatus,
    version: &str,
    pages: &[&str],
) {
    let inputs = pages
        .iter()
        .enumerate()
        .map(|(index, content)| DocumentTextPageInput {
            page_number: i32::try_from(index + 1).expect("page number"),
            content,
            confidence: Some(90.0),
            width: Some(100),
            height: Some(100),
        })
        .collect::<Vec<_>>();
    replace_document_text(
        pool,
        &UpsertDocumentText {
            node_id,
            source,
            status,
            language: "eng",
            engine_name: "tesseract",
            engine_version: version,
            page_count: Some(i32::try_from(pages.len()).expect("page count")),
            mean_confidence: Some(90.0),
            char_count: i32::try_from(pages.iter().map(|page| page.len()).sum::<usize>())
                .expect("character count"),
            warnings: &[],
            duration_ms: Some(20),
        },
        &inputs,
    )
    .await
    .expect("store OCR text");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn engine_status_events_reprocessing_and_page_search_are_durable() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let mut lock = pool.begin().await.expect("begin OCR test lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(0x4f43_5200_i64)
        .execute(&mut *lock)
        .await
        .expect("acquire OCR test lock");
    set_ocr_engine_state(&pool, "tesseract", "5.5-current", "eng")
        .await
        .expect("set OCR engine");
    let engine = get_ocr_engine_state(&pool)
        .await
        .expect("get OCR engine")
        .expect("OCR engine state");
    assert_eq!(engine.engine_version, "5.5-current");

    let complete = create_file(&pool, &format!("ocr-complete-{}.pdf", Uuid::new_v4())).await;
    let failed = create_file(&pool, &format!("ocr-failed-{}.png", Uuid::new_v4())).await;
    let pending = create_file(&pool, &format!("ocr-pending-{}.png", Uuid::new_v4())).await;
    let running = create_file(&pool, &format!("ocr-running-{}.png", Uuid::new_v4())).await;
    let trashed = create_file(&pool, &format!("ocr-trashed-{}.pdf", Uuid::new_v4())).await;
    let marker = format!("telescope{}", Uuid::new_v4().simple());
    let complete_marker = format!("unique {marker}");
    let trashed_marker = format!("hidden {marker}");
    sqlx::query("DELETE FROM jobs WHERE target_node_id = ANY($1)")
        .bind([complete, failed, pending, running, trashed])
        .execute(&pool)
        .await
        .expect("clear automatic OCR jobs before controlled status setup");
    store_text(
        &pool,
        complete,
        DocumentTextSource::Ocr,
        DocumentTextStatus::Completed,
        "5.4-old",
        &["alpha sibling page", &complete_marker],
    )
    .await;
    store_text(
        &pool,
        failed,
        DocumentTextSource::Ocr,
        DocumentTextStatus::Failed,
        "5.5-current",
        &[],
    )
    .await;
    store_text(
        &pool,
        trashed,
        DocumentTextSource::Embedded,
        DocumentTextStatus::Completed,
        "server",
        &[&trashed_marker],
    )
    .await;
    trash_node(&pool, trashed)
        .await
        .expect("trash search fixture");
    enqueue_job(&pool, JobType::Ocr, pending, -10)
        .await
        .expect("enqueue pending");
    enqueue_job(&pool, JobType::Ocr, running, i32::MAX)
        .await
        .expect("enqueue running");
    claim_job(&pool, JobType::Ocr, "ocr-operations", Duration::minutes(1))
        .await
        .expect("claim running")
        .expect("running OCR");

    let counts = get_ocr_status_counts(&pool).await.expect("OCR counts");
    assert!(counts.pending >= 1);
    assert!(counts.running >= 1);
    assert!(counts.completed >= 1);
    assert!(counts.failed >= 1);
    assert!(counts.skipped >= 1);
    assert_eq!(counts.remaining, counts.pending + counts.running);

    let event = append_ocr_event(&pool, complete, "completed", Some(2), Some(90.0), None)
        .await
        .expect("append OCR event");
    assert_eq!(
        list_ocr_events_after(&pool, event.id - 1, 10)
            .await
            .expect("list OCR events")
            .last()
            .expect("event")
            .id,
        event.id
    );

    let matches = search_document_text(&pool, &marker, false, None, 10)
        .await
        .expect("search active OCR text");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].node_id, complete);
    assert_eq!(matches[0].page_number, 2);
    assert!(matches[0].snippet.contains("<<strife>>"));
    assert_eq!(
        search_document_text(&pool, &marker, true, None, 10)
            .await
            .expect("search including trash")
            .len(),
        2
    );

    let failed_enqueued = enqueue_ocr_reprocessing(&pool, &OcrReprocessScope::Failed, 100)
        .await
        .expect("reprocess failed");
    assert!(failed_enqueued >= 1);
    assert_eq!(
        enqueue_ocr_reprocessing(&pool, &OcrReprocessScope::Failed, 100)
            .await
            .expect("repeat failed reprocess"),
        0
    );
    sqlx::query(
        "UPDATE document_text SET updated_at = now() - interval '100 years' WHERE node_id = $1",
    )
    .bind(complete)
    .execute(&pool)
    .await
    .expect("prioritize controlled version-mismatch fixture");
    assert_eq!(
        enqueue_ocr_reprocessing(
            &pool,
            &OcrReprocessScope::VersionMismatch("5.5-current".to_owned()),
            1,
        )
        .await
        .expect("bounded version reprocess"),
        1
    );

    for node_id in [complete, failed, pending, running, trashed] {
        sqlx::query("DELETE FROM file_objects WHERE node_id = $1")
            .bind(node_id)
            .execute(&pool)
            .await
            .expect("delete OCR object");
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(node_id)
            .execute(&pool)
            .await
            .expect("delete OCR node");
    }
}
