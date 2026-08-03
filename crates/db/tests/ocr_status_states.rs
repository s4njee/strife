use chrono::Duration;
use sqlx::PgPool;
use strife_db::{
    DocumentTextSource, DocumentTextStatus, JobType, ROOT_NODE_ID, UpsertDocumentText, claim_job,
    enqueue_job, get_ocr_status_counts, replace_document_text,
};
use uuid::Uuid;

async fn create_node(pool: &PgPool, label: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(id)
        .bind(ROOT_NODE_ID)
        .bind(format!("{label}-{id}.png"))
        .execute(pool)
        .await
        .expect("create OCR status node");
    id
}

async fn set_status(
    pool: &PgPool,
    node_id: Uuid,
    source: DocumentTextSource,
    status: DocumentTextStatus,
) {
    replace_document_text(
        pool,
        &UpsertDocumentText {
            node_id,
            source,
            status,
            language: "eng",
            engine_name: "tesseract",
            engine_version: "status-fixture",
            page_count: Some(0),
            mean_confidence: None,
            char_count: 0,
            warnings: &[],
            duration_ms: Some(1),
        },
        &[],
    )
    .await
    .expect("set OCR status");
}

#[sqlx::test(migrations = "./migrations")]
async fn status_counts_cover_empty_mixed_and_all_complete(pool: PgPool) {
    let empty = get_ocr_status_counts(&pool)
        .await
        .expect("empty OCR counts");
    assert_eq!(empty.pending, 0);
    assert_eq!(empty.running, 0);
    assert_eq!(empty.completed, 0);
    assert_eq!(empty.failed, 0);
    assert_eq!(empty.skipped, 0);
    assert_eq!(empty.unsupported, 0);
    assert_eq!(empty.remaining, 0);

    let completed = create_node(&pool, "completed").await;
    let failed = create_node(&pool, "failed").await;
    let skipped = create_node(&pool, "embedded").await;
    let unsupported = create_node(&pool, "unsupported").await;
    let pending = create_node(&pool, "pending").await;
    let running = create_node(&pool, "running").await;
    set_status(
        &pool,
        completed,
        DocumentTextSource::Ocr,
        DocumentTextStatus::Completed,
    )
    .await;
    set_status(
        &pool,
        failed,
        DocumentTextSource::Ocr,
        DocumentTextStatus::Failed,
    )
    .await;
    set_status(
        &pool,
        skipped,
        DocumentTextSource::Embedded,
        DocumentTextStatus::Completed,
    )
    .await;
    set_status(
        &pool,
        unsupported,
        DocumentTextSource::Ocr,
        DocumentTextStatus::Unsupported,
    )
    .await;
    enqueue_job(&pool, JobType::Ocr, pending, -10)
        .await
        .expect("enqueue pending OCR");
    enqueue_job(&pool, JobType::Ocr, running, i32::MAX)
        .await
        .expect("enqueue running OCR");
    claim_job(&pool, JobType::Ocr, "status-fixture", Duration::minutes(1))
        .await
        .expect("claim running OCR")
        .expect("running OCR job");

    let mixed = get_ocr_status_counts(&pool)
        .await
        .expect("mixed OCR counts");
    assert_eq!(mixed.pending, 1);
    assert_eq!(mixed.running, 1);
    assert_eq!(mixed.completed, 1);
    assert_eq!(mixed.failed, 1);
    assert_eq!(mixed.skipped, 1);
    assert_eq!(mixed.unsupported, 1);
    assert_eq!(mixed.remaining, 2);

    sqlx::query("DELETE FROM jobs WHERE job_type = 'ocr'")
        .execute(&pool)
        .await
        .expect("clear OCR jobs");
    for node_id in [failed, skipped, unsupported, pending, running] {
        set_status(
            &pool,
            node_id,
            DocumentTextSource::Ocr,
            DocumentTextStatus::Completed,
        )
        .await;
    }
    let all_complete = get_ocr_status_counts(&pool)
        .await
        .expect("all-complete OCR counts");
    assert_eq!(all_complete.pending, 0);
    assert_eq!(all_complete.running, 0);
    assert_eq!(all_complete.completed, 6);
    assert_eq!(all_complete.failed, 0);
    assert_eq!(all_complete.skipped, 0);
    assert_eq!(all_complete.unsupported, 0);
    assert_eq!(all_complete.remaining, 0);
}
