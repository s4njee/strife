use std::{path::PathBuf, sync::Arc, time::Duration as StdDuration};

use chrono::Duration;
use sqlx::PgPool;
use strife_db::{
    EmailExtractionStatus, JobState, JobType, ROOT_NODE_ID, claim_job, complete_job,
    create_file_object, enqueue_job, finalize_file_object, get_email_message, get_job,
    list_email_addresses, list_email_attachments, list_email_headers, list_email_labels,
    trash_node,
};
use strife_media::EmailParseLimits;
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use strife_worker::{EmailHandler, EmailSettings, JobHandler};
use uuid::Uuid;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../media/tests/fixtures/email")
        .join(name);
    std::fs::read(path).expect("read email fixture")
}

struct Harness {
    handler: EmailHandler,
    storage: Arc<dyn StorageBackend>,
    root: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn harness(pool: &PgPool) -> Harness {
    let root = std::env::temp_dir().join(format!("strife-email-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create storage root");
    let storage: Arc<dyn StorageBackend> = Arc::new(
        LocalFsBackend::new(root.clone())
            .await
            .expect("create storage backend"),
    );
    Harness {
        handler: EmailHandler::new(pool.clone(), storage.clone()),
        storage,
        root,
    }
}

/// Seeds a finalized `.eml` node whose original bytes are in managed storage.
async fn seed_message(pool: &PgPool, harness: &Harness, name: &str, bytes: &[u8]) -> Uuid {
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
        .bind(format!("{name}-{node_id}"))
        .execute(pool)
        .await
        .expect("create email node");
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

async fn run(pool: &PgPool, harness: &Harness, node_id: Uuid) -> strife_db::JobRecord {
    enqueue_job(pool, JobType::EmailExtraction, node_id, 0)
        .await
        .expect("enqueue email job")
        .expect("new email job");
    let job = claim_job(
        pool,
        JobType::EmailExtraction,
        "email-test",
        Duration::minutes(1),
    )
    .await
    .expect("claim email job")
    .expect("leased email job");
    harness
        .handler
        .handle(&job)
        .await
        .expect("handle email job");
    // The real processor loop completes a leased job after a successful
    // handler run. Without it the active-job uniqueness rule — correctly —
    // refuses the next enqueue for this node.
    let current = get_job(pool, job.id).await.expect("reload job");
    if current.is_some_and(|record| record.state == JobState::Leased) {
        complete_job(pool, job.id)
            .await
            .expect("complete email job");
    }
    job
}

#[sqlx::test(migrations = "../db/migrations")]
async fn plain_message_is_parsed_into_a_complete_projection(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(
        &pool,
        &harness,
        "plain",
        &fixture("mixed-with-attachment.eml"),
    )
    .await;
    run(&pool, &harness, node_id).await;

    let message = get_email_message(&pool, node_id)
        .await
        .expect("load message")
        .expect("message exists");
    assert_eq!(message.status, EmailExtractionStatus::Completed);
    assert_eq!(message.subject.as_deref(), Some("Report attached"));
    assert_eq!(message.attachment_count, 1);
    assert!(message.duration_ms.is_some(), "duration was not recorded");
    assert!(!message.content_hash.unwrap_or_default().is_empty());

    assert_eq!(list_email_addresses(&pool, node_id).await.unwrap().len(), 3);
    assert!(!list_email_headers(&pool, node_id).await.unwrap().is_empty());
    let attachments = list_email_attachments(&pool, node_id).await.unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].filename.as_deref(), Some("report.pdf"));
}

#[sqlx::test(migrations = "../db/migrations")]
async fn gmail_labels_and_thread_id_are_persisted(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, "gmail", &fixture("gmail-labels.eml")).await;
    run(&pool, &harness, node_id).await;

    let message = get_email_message(&pool, node_id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(
        message.provider_thread_id.as_deref(),
        Some("1598765432109876543")
    );
    assert_eq!(
        list_email_labels(&pool, node_id).await.unwrap(),
        vec![
            "Important".to_owned(),
            "Inbox".to_owned(),
            "Work/Reports".to_owned()
        ]
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn reparsing_is_idempotent_and_creates_no_duplicates(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, "idempotent", &fixture("plain-text.eml")).await;
    run(&pool, &harness, node_id).await;
    let first = get_email_message(&pool, node_id)
        .await
        .unwrap()
        .expect("first parse");
    let first_addresses = list_email_addresses(&pool, node_id).await.unwrap().len();

    // Same node, same parser version: the projection must be replaced, not
    // appended to, and must produce identical normalized values.
    run(&pool, &harness, node_id).await;
    let second = get_email_message(&pool, node_id)
        .await
        .unwrap()
        .expect("second parse");

    assert_eq!(first.subject, second.subject);
    assert_eq!(first.content_hash, second.content_hash);
    assert_eq!(first.body_text, second.body_text);
    assert_eq!(
        list_email_addresses(&pool, node_id).await.unwrap().len(),
        first_addresses,
        "reparsing duplicated address rows"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_non_email_file_is_recorded_unsupported_rather_than_failed(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(
        &pool,
        &harness,
        "not-email",
        b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n%%EOF\n",
    )
    .await;
    run(&pool, &harness, node_id).await;

    let message = get_email_message(&pool, node_id)
        .await
        .unwrap()
        .expect("projection exists");
    assert_eq!(message.status, EmailExtractionStatus::Unsupported);
    assert!(
        message
            .warnings
            .iter()
            .any(|warning| warning.contains("does not support")),
        "unsupported reason was not recorded: {:?}",
        message.warnings
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_trashed_file_is_skipped_and_the_job_is_not_failed(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, "trashed", &fixture("plain-text.eml")).await;
    trash_node(&pool, node_id).await.expect("trash node");
    let job = run(&pool, &harness, node_id).await;

    let message = get_email_message(&pool, node_id)
        .await
        .unwrap()
        .expect("projection exists");
    assert_eq!(message.status, EmailExtractionStatus::Skipped);
    let reloaded = get_job(&pool, job.id).await.unwrap().expect("job exists");
    assert_eq!(
        reloaded.state,
        JobState::Skipped,
        "a trashed file must not consume the retry budget"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_node_deleted_mid_flight_fails_without_orphan_rows(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, "vanishing", &fixture("plain-text.eml")).await;
    enqueue_job(&pool, JobType::EmailExtraction, node_id, 0)
        .await
        .expect("enqueue")
        .expect("job");
    let job = claim_job(
        &pool,
        JobType::EmailExtraction,
        "email-test",
        Duration::minutes(1),
    )
    .await
    .expect("claim")
    .expect("leased");

    // The node disappears between claim and handling.
    sqlx::query("DELETE FROM file_objects WHERE node_id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("detach file object");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("delete node");

    let error = harness
        .handler
        .handle(&job)
        .await
        .expect_err("a missing node must fail cleanly");
    assert!(format!("{error:#}").contains("no longer exists"));
    assert!(
        get_email_message(&pool, node_id).await.unwrap().is_none(),
        "an orphan projection was written for a deleted node"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_oversized_message_fails_terminally_without_burning_retries(pool: PgPool) {
    let harness = harness(&pool).await;
    let handler =
        EmailHandler::new(pool.clone(), harness.storage.clone()).with_settings(EmailSettings {
            limits: EmailParseLimits {
                max_source_bytes: 16,
                ..EmailParseLimits::default()
            },
            attachments: strife_media::AttachmentLimits::default(),
            file_timeout: StdDuration::from_secs(30),
        });
    let node_id = seed_message(&pool, &harness, "oversized", &fixture("plain-text.eml")).await;
    enqueue_job(&pool, JobType::EmailExtraction, node_id, 0)
        .await
        .expect("enqueue")
        .expect("job");
    let job = claim_job(
        &pool,
        JobType::EmailExtraction,
        "email-test",
        Duration::minutes(1),
    )
    .await
    .expect("claim")
    .expect("leased");

    handler
        .handle(&job)
        .await
        .expect("limit failure is handled");

    let message = get_email_message(&pool, node_id)
        .await
        .unwrap()
        .expect("projection exists");
    assert_eq!(message.status, EmailExtractionStatus::Failed);
    assert!(
        message
            .warnings
            .iter()
            .any(|warning| warning.contains("size limit exceeded")),
        "the limit that was hit must be named: {:?}",
        message.warnings
    );
    let reloaded = get_job(&pool, job.id).await.unwrap().expect("job exists");
    assert_eq!(
        reloaded.state,
        JobState::Failed,
        "a limit failure must be terminal, not retried"
    );
    assert!(
        reloaded
            .last_error
            .as_deref()
            .is_some_and(|error| !error.contains('\n')),
        "the API-visible error must stay single-line"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn every_fixture_in_the_corpus_parses_through_the_handler(pool: PgPool) {
    let harness = harness(&pool).await;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../media/tests/fixtures/email");
    for entry in std::fs::read_dir(dir).expect("read fixture dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|extension| extension != "eml") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).expect("read fixture");
        let node_id = seed_message(&pool, &harness, "corpus", &bytes).await;
        run(&pool, &harness, node_id).await;
        let message = get_email_message(&pool, node_id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: no projection was written"));
        assert_eq!(
            message.status,
            EmailExtractionStatus::Completed,
            "{name}: expected a completed projection"
        );
    }
}
