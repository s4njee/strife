//! Attachment materialization: bounded, deterministic, and idempotent.
//!
//! The `.eml` original is canonical and every artifact here is disposable, so
//! the properties worth pinning down are that a rerun replaces rather than
//! accumulates, that a bad part cannot take the message down with it, and that
//! a sender-supplied filename never reaches a storage path.

use std::{path::PathBuf, sync::Arc, time::Duration as StdDuration};

use chrono::Duration;
use sqlx::PgPool;
use strife_db::{
    EmailArtifactState, EmailExtractionStatus, JobState, JobType, ROOT_NODE_ID, claim_job,
    complete_job, create_file_object, email_attachment_artifact_id, enqueue_job,
    finalize_file_object, get_email_message, get_job, list_email_attachment_artifacts,
};
use strife_media::{AttachmentLimits, EmailParseLimits};
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

async fn harness_with(pool: &PgPool, attachments: AttachmentLimits) -> Harness {
    let root = std::env::temp_dir().join(format!("strife-attach-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create storage root");
    let storage: Arc<dyn StorageBackend> = Arc::new(
        LocalFsBackend::new(root.clone())
            .await
            .expect("create storage backend"),
    );
    let handler = EmailHandler::new(pool.clone(), storage.clone()).with_settings(EmailSettings {
        limits: EmailParseLimits::default(),
        attachments,
        file_timeout: StdDuration::from_secs(30),
    });
    Harness {
        handler,
        storage,
        root,
    }
}

async fn harness(pool: &PgPool) -> Harness {
    harness_with(pool, AttachmentLimits::default()).await
}

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

async fn run(pool: &PgPool, harness: &Harness, node_id: Uuid) {
    enqueue_job(pool, JobType::EmailExtraction, node_id, 0)
        .await
        .expect("enqueue email job")
        .expect("new email job");
    let job = claim_job(
        pool,
        JobType::EmailExtraction,
        "attach-test",
        Duration::minutes(1),
    )
    .await
    .expect("claim email job")
    .expect("leased email job");
    harness.handler.handle(&job).await.expect("handle job");
    let current = get_job(pool, job.id).await.expect("reload job");
    if current.is_some_and(|record| record.state == JobState::Leased) {
        complete_job(pool, job.id).await.expect("complete job");
    }
}

async fn read_artifact(harness: &Harness, storage_key: &str) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let id = Uuid::parse_str(storage_key).expect("artifact key is a uuid");
    let mut reader = harness
        .storage
        .get_stream(StorageKey::artifact(id))
        .await
        .expect("open artifact");
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.expect("read artifact");
    bytes
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_binary_attachment_is_written_with_a_deterministic_key(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(
        &pool,
        &harness,
        "mixed",
        &fixture("mixed-with-attachment.eml"),
    )
    .await;
    run(&pool, &harness, node_id).await;

    let artifacts = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    assert_eq!(artifacts.len(), 1);
    let artifact = &artifacts[0];
    assert_eq!(artifact.state, EmailArtifactState::Ready);
    assert_eq!(artifact.media_type, "application/pdf");
    assert!(artifact.byte_size > 0);
    assert_eq!(artifact.depth, 0);

    // The key is a pure function of message and MIME part identity: nothing
    // about the sender's filename participates.
    let expected = email_attachment_artifact_id(node_id, &artifact.part_path);
    assert_eq!(
        artifact.storage_key.as_deref(),
        Some(&*expected.to_string())
    );

    // The stored bytes are the decoded attachment, and the recorded checksum
    // describes what was actually written.
    let stored = read_artifact(&harness, artifact.storage_key.as_deref().unwrap()).await;
    assert_eq!(
        i64::try_from(stored.len()).expect("size"),
        artifact.byte_size
    );
    let digest = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(&stored))
    };
    assert_eq!(artifact.checksum_sha256.as_deref(), Some(digest.as_str()));
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_inline_part_is_materialized_like_any_other_attachment(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, "inline", &fixture("inline-cid-image.eml")).await;
    run(&pool, &harness, node_id).await;

    let artifacts = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].state, EmailArtifactState::Ready);
    assert_eq!(artifacts[0].media_type, "image/png");
    // An inline image is what a `cid:` reference resolves to, so it has to be
    // stored rather than treated as decoration.
    assert!(artifacts[0].byte_size > 0);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn attachments_sharing_a_filename_get_separate_artifacts(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(
        &pool,
        &harness,
        "dupnames",
        &fixture("duplicate-attachment-names.eml"),
    )
    .await;
    run(&pool, &harness, node_id).await;

    let artifacts = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    // Two attachments called invoice.pdf must not collide. Keying on MIME part
    // path rather than filename is what keeps them apart; a filename-derived
    // key would have silently kept only one of them.
    assert_eq!(artifacts.len(), 2);
    assert_ne!(artifacts[0].part_path, artifacts[1].part_path);
    assert_ne!(artifacts[0].storage_key, artifacts[1].storage_key);

    let first = read_artifact(&harness, artifacts[0].storage_key.as_deref().unwrap()).await;
    let second = read_artifact(&harness, artifacts[1].storage_key.as_deref().unwrap()).await;
    assert_ne!(first, second, "one attachment overwrote the other");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_nested_message_is_stored_whole_and_not_unpacked(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(&pool, &harness, "nested", &fixture("nested-rfc822.eml")).await;
    run(&pool, &harness, node_id).await;

    let artifacts = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    assert!(!artifacts.is_empty(), "nested message was not materialized");
    let nested = artifacts
        .iter()
        .find(|artifact| artifact.is_message)
        .expect("a nested message part");
    assert_eq!(nested.state, EmailArtifactState::Ready);

    // Its bytes are stored as one opaque artifact. Nothing inside it becomes a
    // top-level Strife file, which is what silent import would look like.
    let node_count: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE parent_id = $1")
        .bind(ROOT_NODE_ID)
        .fetch_one(&pool)
        .await
        .expect("count nodes");
    assert_eq!(node_count, 1, "a nested message leaked into the file tree");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_oversized_part_is_skipped_without_failing_the_message(pool: PgPool) {
    let harness = harness_with(
        &pool,
        AttachmentLimits {
            max_part_bytes: 4,
            ..AttachmentLimits::default()
        },
    )
    .await;
    let node_id = seed_message(
        &pool,
        &harness,
        "oversized",
        &fixture("mixed-with-attachment.eml"),
    )
    .await;
    run(&pool, &harness, node_id).await;

    let artifacts = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    assert!(
        artifacts.is_empty(),
        "an over-limit part was written anyway"
    );

    // The message itself parsed fine and stays searchable. Refusing to store a
    // large PDF is not a reason to lose the message's text.
    let message = get_email_message(&pool, node_id)
        .await
        .expect("load message")
        .expect("message exists");
    assert_eq!(message.status, EmailExtractionStatus::Completed);
    assert!(
        message
            .warnings
            .iter()
            .any(|warning| warning.contains("limit")),
        "no warning explained the skipped attachment: {:?}",
        message.warnings
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_message_total_limit_stops_writing_further_parts(pool: PgPool) {
    let harness = harness_with(
        &pool,
        AttachmentLimits {
            // Enough for the first attachment but not for both.
            max_message_bytes: 20,
            ..AttachmentLimits::default()
        },
    )
    .await;
    let node_id = seed_message(
        &pool,
        &harness,
        "total",
        &fixture("duplicate-attachment-names.eml"),
    )
    .await;
    run(&pool, &harness, node_id).await;

    let artifacts = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    assert_eq!(artifacts.len(), 1, "the total limit was not enforced");
    let message = get_email_message(&pool, node_id)
        .await
        .expect("load message")
        .expect("message exists");
    assert_eq!(message.status, EmailExtractionStatus::Completed);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_malformed_transfer_encoding_does_not_fail_the_message(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(
        &pool,
        &harness,
        "badtransfer",
        &fixture("malformed-transfer-encoding.eml"),
    )
    .await;
    run(&pool, &harness, node_id).await;

    // Whatever the decoder made of the broken part, the message is completed
    // and any artifact row is in a terminal state rather than stuck pending.
    let message = get_email_message(&pool, node_id)
        .await
        .expect("load message")
        .expect("message exists");
    assert_eq!(message.status, EmailExtractionStatus::Completed);
    for artifact in list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts")
    {
        assert_ne!(
            artifact.state,
            EmailArtifactState::Pending,
            "artifact {} was left pending",
            artifact.part_path
        );
    }
}

#[sqlx::test(migrations = "../db/migrations")]
async fn rerunning_replaces_artifacts_rather_than_accumulating_them(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(
        &pool,
        &harness,
        "rerun",
        &fixture("mixed-with-attachment.eml"),
    )
    .await;
    run(&pool, &harness, node_id).await;

    let first = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    assert_eq!(first.len(), 1);

    run(&pool, &harness, node_id).await;

    let second = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    assert_eq!(second.len(), 1, "a rerun accumulated a second artifact");
    // Identity is stable across runs, so the same object is replaced in place.
    assert_eq!(first[0].id, second[0].id);
    assert_eq!(first[0].storage_key, second[0].storage_key);
    assert_eq!(first[0].checksum_sha256, second[0].checksum_sha256);
    assert!(
        second[0].updated_at >= first[0].updated_at,
        "the rerun did not touch the row"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn artifact_bytes_are_reclaimed_when_the_message_is_deleted(pool: PgPool) {
    let harness = harness(&pool).await;
    let node_id = seed_message(
        &pool,
        &harness,
        "purge",
        &fixture("mixed-with-attachment.eml"),
    )
    .await;
    run(&pool, &harness, node_id).await;

    let artifacts = list_email_attachment_artifacts(&pool, node_id)
        .await
        .expect("list artifacts");
    let storage_key = artifacts[0].storage_key.clone().expect("stored key");

    strife_db::trash_node(&pool, node_id).await.expect("trash");
    let keys = strife_db::list_storage_keys_for_deletion(&pool, node_id)
        .await
        .expect("list deletion keys");
    // Permanent deletion has to know about attachment artifacts, or their bytes
    // outlive the message they belong to.
    assert!(
        keys.iter()
            .any(|entry| entry.artifact_storage_keys.contains(&storage_key)),
        "attachment artifact was not scheduled for reclamation"
    );
}
