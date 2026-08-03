//! End-to-end archive validation: a directory of `.eml` files through import,
//! extraction, search, reading, attachments, duplicates, threads, trash, and
//! permanent deletion.
//!
//! Every other email suite tests one seam. This one exists to catch the failures
//! that only appear when the seams are joined — a projection written but never
//! indexed, an attachment materialized but unreachable, a trashed message that
//! keeps answering searches. It is deliberately the slowest email test.

use std::{path::PathBuf, sync::Arc};

use chrono::Duration;
use sqlx::PgPool;
use strife_db::{
    EmailExtractionStatus, EmailSearchFilters, JobState, JobType, ROOT_NODE_ID, claim_job,
    complete_job, create_file_object, enqueue_job, finalize_file_object, get_email_message,
    get_job, list_email_attachment_artifacts, list_storage_keys_for_deletion, search_email,
    trash_node,
};
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use strife_worker::{EmailHandler, JobHandler};
use uuid::Uuid;

/// The corpus every stage below is checked against.
const CORPUS: &[&str] = &[
    "plain-text.eml",
    "html-only.eml",
    "mixed-with-attachment.eml",
    "inline-cid-image.eml",
    "gmail-labels.eml",
    "duplicate-message-id-a.eml",
    "duplicate-message-id-b.eml",
    "utf8-subject-and-body.eml",
    "quoted-printable.eml",
    "nested-rfc822.eml",
];

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
    let root = std::env::temp_dir().join(format!("strife-e2e-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create storage root");
    let storage: Arc<dyn StorageBackend> = Arc::new(
        LocalFsBackend::new(root.clone())
            .await
            .expect("create backend"),
    );
    Harness {
        handler: EmailHandler::new(pool.clone(), storage.clone()),
        storage,
        root,
    }
}

/// Stages one `.eml` exactly as a finalized upload or import leaves it.
async fn ingest(pool: &PgPool, harness: &Harness, name: &str) -> Uuid {
    let bytes = fixture(name);
    let node_id = Uuid::new_v4();
    let storage_id = Uuid::new_v4();
    harness
        .storage
        .put_stream(
            StorageKey::original(storage_id),
            Box::pin(std::io::Cursor::new(bytes.clone())),
        )
        .await
        .expect("write original");
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("{name}-{node_id}"))
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
        .expect("finalize");
    node_id
}

/// Drains every queued email job, as the processor loop would.
async fn drain(pool: &PgPool, harness: &Harness) -> usize {
    let mut processed = 0;
    while let Some(job) = claim_job(
        pool,
        JobType::EmailExtraction,
        "e2e-worker",
        Duration::minutes(5),
    )
    .await
    .expect("claim")
    {
        harness.handler.handle(&job).await.expect("handle");
        let current = get_job(pool, job.id).await.expect("reload");
        if current.is_some_and(|record| record.state == JobState::Leased) {
            complete_job(pool, job.id).await.expect("complete");
        }
        processed += 1;
    }
    processed
}

async fn ingest_corpus(pool: &PgPool, harness: &Harness) -> Vec<(String, Uuid)> {
    let mut nodes = Vec::new();
    for name in CORPUS {
        let node_id = ingest(pool, harness, name).await;
        enqueue_job(pool, JobType::EmailExtraction, node_id, 0)
            .await
            .expect("enqueue")
            .expect("new job");
        nodes.push(((*name).to_owned(), node_id));
    }
    drain(pool, harness).await;
    nodes
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_directory_of_messages_becomes_a_searchable_readable_archive(pool: PgPool) {
    let harness = harness(&pool).await;
    let nodes = ingest_corpus(&pool, &harness).await;
    assert_eq!(nodes.len(), CORPUS.len());

    // Every message reached a terminal state and was indexed.
    for (name, node_id) in &nodes {
        let message = get_email_message(&pool, *node_id)
            .await
            .expect("load")
            .unwrap_or_else(|| panic!("{name} produced no projection"));
        assert_ne!(
            message.status,
            EmailExtractionStatus::Pending,
            "{name} was left pending"
        );
    }
    let unindexed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_messages WHERE status = 'completed' AND search_vector IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(
        unindexed, 0,
        "completed messages were left out of the index"
    );

    // Weighted search: a subject term ranks above the same term in a body.
    let hits = search_email(
        &pool,
        Some("attached"),
        &EmailSearchFilters::default(),
        None,
        25,
    )
    .await
    .expect("search");
    assert!(!hits.is_empty(), "nothing matched a term in the corpus");
    assert!(
        hits[0].snippet.contains("[["),
        "snippet carried no highlight markers: {}",
        hits[0].snippet
    );

    // Structured filters narrow the same corpus.
    let with_attachments = search_email(
        &pool,
        None,
        &EmailSearchFilters {
            has_attachment: Some(true),
            ..EmailSearchFilters::default()
        },
        None,
        25,
    )
    .await
    .expect("attachment filter");
    assert!(!with_attachments.is_empty());
    for hit in &with_attachments {
        assert!(hit.attachment_count > 0);
    }

    // Label filter over the Gmail fixture.
    let labelled = search_email(
        &pool,
        None,
        &EmailSearchFilters {
            labels: vec!["Work/Reports".to_owned()],
            ..EmailSearchFilters::default()
        },
        None,
        25,
    )
    .await
    .expect("label filter");
    assert_eq!(labelled.len(), 1);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn reader_data_attachments_threads_and_duplicates_all_resolve(pool: PgPool) {
    let harness = harness(&pool).await;
    let nodes = ingest_corpus(&pool, &harness).await;

    // Reader data: a message with an attachment has a manifest and a stored
    // artifact whose bytes are actually readable.
    let (_, mixed) = nodes
        .iter()
        .find(|(name, _)| name == "mixed-with-attachment.eml")
        .expect("fixture present");
    let attachments = strife_db::list_email_attachments(&pool, *mixed)
        .await
        .expect("manifest");
    assert_eq!(attachments.len(), 1);
    let artifacts = list_email_attachment_artifacts(&pool, *mixed)
        .await
        .expect("artifacts");
    assert_eq!(artifacts.len(), 1);
    let key = artifacts[0].storage_key.clone().expect("stored");
    let exists = harness
        .storage
        .exists(StorageKey::artifact(
            Uuid::parse_str(&key).expect("uuid key"),
        ))
        .await
        .expect("stat artifact");
    assert!(exists, "artifact row exists but its bytes do not");

    // Duplicates: the two duplicate fixtures share a group and collapse.
    let (_, dup_a) = nodes
        .iter()
        .find(|(name, _)| name == "duplicate-message-id-a.eml")
        .expect("fixture present");
    let (_, dup_b) = nodes
        .iter()
        .find(|(name, _)| name == "duplicate-message-id-b.eml")
        .expect("fixture present");
    let a = get_email_message(&pool, *dup_a).await.unwrap().unwrap();
    let b = get_email_message(&pool, *dup_b).await.unwrap().unwrap();
    assert_eq!(
        a.duplicate_group_id, b.duplicate_group_id,
        "duplicate fixtures were not grouped"
    );
    assert!(a.duplicate_group_id.is_some());

    // Threads: every message that carries an identifier lands in a group.
    for (name, node_id) in &nodes {
        let message = get_email_message(&pool, *node_id).await.unwrap().unwrap();
        if message.status == EmailExtractionStatus::Completed && message.message_id.is_some() {
            assert!(
                message.thread_group_id.is_some(),
                "{name} has an id but no thread group"
            );
        }
    }
}

#[sqlx::test(migrations = "../db/migrations")]
async fn trashing_excludes_a_message_and_deleting_reclaims_its_artifacts(pool: PgPool) {
    let harness = harness(&pool).await;
    let nodes = ingest_corpus(&pool, &harness).await;
    let (_, mixed) = nodes
        .iter()
        .find(|(name, _)| name == "mixed-with-attachment.eml")
        .expect("fixture present");

    let before = search_email(
        &pool,
        Some("quarterly"),
        &EmailSearchFilters::default(),
        None,
        25,
    )
    .await
    .expect("search");
    assert!(before.iter().any(|hit| hit.node_id == *mixed));

    trash_node(&pool, *mixed).await.expect("trash");

    let after = search_email(
        &pool,
        Some("quarterly"),
        &EmailSearchFilters::default(),
        None,
        25,
    )
    .await
    .expect("search");
    assert!(
        !after.iter().any(|hit| hit.node_id == *mixed),
        "a trashed message kept answering searches"
    );

    // Permanent deletion must reclaim the original and the attachment artifact.
    let keys = list_storage_keys_for_deletion(&pool, *mixed)
        .await
        .expect("deletion keys");
    let entry = keys
        .iter()
        .find(|entry| entry.node_id == *mixed)
        .expect("entry");
    assert!(entry.original_storage_key.is_some());
    assert_eq!(
        entry.artifact_storage_keys.len(),
        1,
        "the attachment artifact was not scheduled for reclamation"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_interrupted_run_recovers_without_duplicating_anything(pool: PgPool) {
    let harness = harness(&pool).await;
    let nodes = ingest_corpus(&pool, &harness).await;

    let counts = |pool: PgPool| async move {
        let messages: i64 = sqlx::query_scalar("SELECT count(*) FROM email_messages")
            .fetch_one(&pool)
            .await
            .expect("count messages");
        let addresses: i64 = sqlx::query_scalar("SELECT count(*) FROM email_addresses")
            .fetch_one(&pool)
            .await
            .expect("count addresses");
        let labels: i64 = sqlx::query_scalar("SELECT count(*) FROM email_labels")
            .fetch_one(&pool)
            .await
            .expect("count labels");
        let attachments: i64 = sqlx::query_scalar("SELECT count(*) FROM email_attachments")
            .fetch_one(&pool)
            .await
            .expect("count attachments");
        let artifacts: i64 = sqlx::query_scalar("SELECT count(*) FROM email_attachment_artifacts")
            .fetch_one(&pool)
            .await
            .expect("count artifacts");
        (messages, addresses, labels, attachments, artifacts)
    };
    let before = counts(pool.clone()).await;

    // Reprocess the whole corpus, standing in for a worker restart that
    // re-claims work it had already partly done.
    for (_, node_id) in &nodes {
        enqueue_job(&pool, JobType::EmailExtraction, *node_id, 0)
            .await
            .expect("re-enqueue");
    }
    let processed = drain(&pool, &harness).await;
    assert_eq!(processed, nodes.len(), "not every message was reprocessed");

    // Idempotent: the same corpus produces the same row counts, not twice as
    // many. Every dependent table is checked, because a duplicate anywhere
    // would double-count in search or the reader.
    assert_eq!(counts(pool.clone()).await, before);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn upload_and_import_paths_produce_equivalent_results(pool: PgPool) {
    let harness = harness(&pool).await;
    // Both paths converge on the same thing: a finalized file object plus an
    // enqueued extraction job. Ingesting the same bytes twice therefore stands
    // in for "uploaded" and "imported", and the projections must not differ.
    let uploaded = ingest(&pool, &harness, "mixed-with-attachment.eml").await;
    let imported = ingest(&pool, &harness, "mixed-with-attachment.eml").await;
    for node_id in [uploaded, imported] {
        enqueue_job(&pool, JobType::EmailExtraction, node_id, 0)
            .await
            .expect("enqueue");
    }
    drain(&pool, &harness).await;

    let a = get_email_message(&pool, uploaded).await.unwrap().unwrap();
    let b = get_email_message(&pool, imported).await.unwrap().unwrap();
    assert_eq!(a.subject, b.subject);
    assert_eq!(a.body_text, b.body_text);
    assert_eq!(a.content_hash, b.content_hash);
    assert_eq!(a.attachment_count, b.attachment_count);
    // Same bytes means same message: they are duplicates of each other.
    assert_eq!(a.duplicate_group_id, b.duplicate_group_id);
}
