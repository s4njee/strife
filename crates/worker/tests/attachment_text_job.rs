//! Attachment text extraction through the real worker.
//!
//! Deliberately limited to routes that need no external service: plain text is
//! read directly and an unsupported type is rejected before any extractor is
//! invoked. PDF, office, and image routing depend on a running Tika server and
//! Tesseract binary, so they are exercised by the adapters' own suites rather
//! than duplicated here behind a service that may not be present.

use std::{path::PathBuf, sync::Arc};

use chrono::Duration;
use sqlx::PgPool;
use strife_db::{
    EmailArtifactState, EmailExtractionStatus, JobState, JobType, ROOT_NODE_ID, claim_job,
    complete_job, create_file_object, enqueue_job, finalize_file_object, get_email_message,
    get_job, list_email_attachment_artifacts, list_email_attachment_text,
};
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use strife_worker::{AttachmentTextHandler, EmailHandler, JobHandler};
use uuid::Uuid;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../media/tests/fixtures/email")
        .join(name);
    std::fs::read(path).expect("read email fixture")
}

struct Harness {
    email: EmailHandler,
    attachments: AttachmentTextHandler,
    storage: Arc<dyn StorageBackend>,
    root: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn harness(pool: &PgPool) -> Harness {
    let root = std::env::temp_dir().join(format!("strife-atext-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create storage root");
    let storage: Arc<dyn StorageBackend> = Arc::new(
        LocalFsBackend::new(root.clone())
            .await
            .expect("create storage backend"),
    );
    Harness {
        email: EmailHandler::new(pool.clone(), storage.clone()),
        // The URL is never reached by these routes; a plain-text attachment is
        // read directly and an unsupported one is refused before extraction.
        attachments: AttachmentTextHandler::new(
            pool.clone(),
            storage.clone(),
            "http://127.0.0.1:1".to_owned(),
        ),
        storage,
        root,
    }
}

async fn seed_message(pool: &PgPool, harness: &Harness, bytes: &[u8]) -> Uuid {
    let node_id = Uuid::new_v4();
    let storage_id = Uuid::new_v4();
    harness
        .storage
        .put_stream(
            StorageKey::original(storage_id),
            Box::pin(std::io::Cursor::new(bytes.to_vec())),
        )
        .await
        .expect("write original");
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("atext-{node_id}.eml"))
        .execute(pool)
        .await
        .expect("create node");
    let file = create_file_object(
        pool,
        storage_id,
        i64::try_from(bytes.len()).expect("size"),
        Some("message/rfc822"),
        None,
    )
    .await
    .expect("create file object");
    finalize_file_object(pool, file.id, node_id)
        .await
        .expect("finalize file object");
    node_id
}

async fn run(pool: &PgPool, job_type: JobType, handler: &dyn JobHandler, node_id: Uuid) {
    // The email job enqueues the attachment job itself, so only the first
    // enqueue here is expected to create work.
    let _ = enqueue_job(pool, job_type, node_id, 0)
        .await
        .expect("enqueue");
    let job = claim_job(pool, job_type, "atext-test", Duration::minutes(5))
        .await
        .expect("claim")
        .expect("leased job");
    handler.handle(&job).await.expect("handle job");
    let current = get_job(pool, job.id).await.expect("reload job");
    if current.is_some_and(|record| record.state == JobState::Leased) {
        complete_job(pool, job.id).await.expect("complete job");
    }
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_readable_attachment_is_indexed_and_an_opaque_one_is_marked_unsupported(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, &fixture("text-and-binary-attachments.eml")).await;

    run(&pool, JobType::EmailExtraction, &harness.email, node_id).await;
    run(
        &pool,
        JobType::AttachmentExtraction,
        &harness.attachments,
        node_id,
    )
    .await;

    let artifacts = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    assert_eq!(artifacts.len(), 2);
    for artifact in &artifacts {
        assert_eq!(artifact.state, EmailArtifactState::Ready);
    }

    let readable = artifacts
        .iter()
        .find(|artifact| artifact.media_type.starts_with("text/"))
        .expect("a text attachment");
    assert_eq!(readable.text_status, EmailExtractionStatus::Completed);
    assert!(readable.text_bytes > 0);

    let opaque = artifacts
        .iter()
        .find(|artifact| !artifact.media_type.starts_with("text/"))
        .expect("an opaque attachment");
    // Unsupported is a fact about the format, not a failure: retrying could
    // never change it, so it must not consume a retry budget or read as broken.
    assert_eq!(opaque.text_status, EmailExtractionStatus::Unsupported);
    assert_eq!(opaque.text_bytes, 0);

    // A message where one attachment worked and another did not is still a
    // completed message.
    let message = get_email_message(&pool, node_id)
        .await
        .expect("load message")
        .expect("message exists");
    assert_eq!(message.status, EmailExtractionStatus::Completed);

    let pages = list_email_attachment_text(&pool, node_id)
        .await
        .expect("list text");
    assert_eq!(pages.len(), 1, "only the readable attachment yields text");
    assert!(
        pages[0].content.contains("perihelion"),
        "attachment text was not captured: {}",
        pages[0].content
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn extraction_is_enqueued_by_the_email_job_when_attachments_are_stored(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, &fixture("mixed-with-attachment.eml")).await;
    run(&pool, JobType::EmailExtraction, &harness.email, node_id).await;

    // Materializing attachments is what makes extraction worth scheduling, so
    // the email job queues it rather than requiring a separate trigger.
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs
         WHERE target_node_id = $1 AND job_type = 'attachment_extraction'",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("count jobs");
    assert_eq!(queued, 1);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_message_without_attachments_queues_no_extraction(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, &fixture("plain-text.eml")).await;
    run(&pool, JobType::EmailExtraction, &harness.email, node_id).await;

    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs
         WHERE target_node_id = $1 AND job_type = 'attachment_extraction'",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("count jobs");
    assert_eq!(queued, 0, "queued extraction for a message with no parts");
}
