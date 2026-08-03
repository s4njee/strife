//! Attachment text in the email index: weighting, provenance, and reprocessing.
//!
//! Uses direct projection writes rather than the worker so the search behaviour
//! is tested without a Tika server or Tesseract in the loop. The worker's own
//! routing is covered separately.

use chrono::{TimeZone, Utc};
use sqlx::PgPool;
use strife_db::{
    AttachmentReprocessScope, EmailAddressInput, EmailAddressRole, EmailArtifactState,
    EmailAttachmentInput, EmailAttachmentTextInput, EmailAttachmentTextOutcome,
    EmailAttachmentTextSource, EmailExtractionStatus, EmailProjection, EmailSearchFilters,
    ROOT_NODE_ID, UpsertEmailAttachmentArtifact, UpsertEmailMessage,
    enqueue_attachment_reprocessing, get_email_attachment_artifact, list_email_attachment_text,
    replace_email_attachment_text, replace_email_projection, search_email,
    upsert_email_attachment_artifact,
};
use uuid::Uuid;

/// Seeds one message with one attachment and returns its node id.
async fn seed(pool: &PgPool, subject: &str, body: &str, filename: &str, part_path: &str) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("att-{node_id}.eml"))
        .execute(pool)
        .await
        .expect("create node");
    replace_email_projection(
        pool,
        &EmailProjection {
            message: UpsertEmailMessage {
                node_id,
                status: EmailExtractionStatus::Completed,
                parser_name: "test",
                parser_version: "1",
                message_id: None,
                normalized_message_id: None,
                in_reply_to: None,
                reference_ids: &[],
                subject: Some(subject),
                normalized_subject: Some(subject),
                sent_at: Some(Utc.with_ymd_and_hms(2020, 6, 1, 9, 0, 0).unwrap()),
                received_at: None,
                body_text: body,
                body_html: None,
                preview_text: body,
                content_hash: None,
                provider_thread_id: None,
                warnings: &[],
                duration_ms: None,
            },
            addresses: &[EmailAddressInput {
                role: EmailAddressRole::From,
                display_name: None,
                address: "ada@example.test",
            }],
            headers: &[],
            labels: &[],
            attachments: &[EmailAttachmentInput {
                part_path,
                filename: Some(filename),
                media_type: "application/pdf",
                disposition: Some("attachment"),
                content_id: None,
                transfer_encoding: Some("base64"),
                decoded_size: Some(2048),
                checksum_sha256: None,
                is_inline: false,
                is_message: false,
                warnings: &[],
            }],
        },
    )
    .await
    .expect("seed projection");

    upsert_email_attachment_artifact(
        pool,
        &UpsertEmailAttachmentArtifact {
            node_id,
            part_path,
            state: EmailArtifactState::Ready,
            storage_key: Some(&Uuid::new_v4().to_string()),
            media_type: "application/pdf",
            byte_size: 2048,
            checksum_sha256: None,
            depth: 0,
            is_message: false,
            materializer_version: "1",
            warnings: &[],
        },
    )
    .await
    .expect("seed artifact");
    node_id
}

async fn store_text(pool: &PgPool, node_id: Uuid, part_path: &str, content: &str) {
    replace_email_attachment_text(
        pool,
        node_id,
        part_path,
        &[EmailAttachmentTextInput {
            page_number: 3,
            content,
            source: EmailAttachmentTextSource::Embedded,
            confidence: None,
        }],
        &EmailAttachmentTextOutcome {
            status: EmailExtractionStatus::Completed,
            extractor_name: Some("tika"),
            extractor_version: Some("1"),
            warnings: &[],
        },
    )
    .await
    .expect("store attachment text");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_message_is_findable_by_the_document_it_carried(pool: PgPool) {
    let node_id = seed(
        &pool,
        "Monthly summary",
        "See attached.",
        "summary.pdf",
        "2",
    )
    .await;
    store_text(&pool, node_id, "2", "The defenestration clause applies.").await;

    let hits = search_email(
        &pool,
        Some("defenestration"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("search");
    assert_eq!(hits.len(), 1, "attachment text was not indexed");
    assert_eq!(hits[0].node_id, node_id);
    // The result explains itself: this term appears nowhere in the message.
    assert!(
        hits[0]
            .match_sources
            .contains(&"attachment_content".to_owned()),
        "{:?}",
        hits[0].match_sources
    );
    assert_eq!(hits[0].matched_attachment.as_deref(), Some("summary.pdf"));
    assert_eq!(hits[0].matched_attachment_page, Some(3));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_body_match_outranks_the_same_term_inside_an_attachment(pool: PgPool) {
    let in_body = seed(
        &pool,
        "Unrelated",
        "The reconciliation is done.",
        "a.pdf",
        "2",
    )
    .await;
    let in_attachment = seed(&pool, "Unrelated too", "Nothing here.", "b.pdf", "2").await;
    store_text(&pool, in_attachment, "2", "The reconciliation is done.").await;

    let hits = search_email(
        &pool,
        Some("reconciliation"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("search");
    assert_eq!(hits.len(), 2);
    // Weight C beats weight D: the message is what is being searched, and a
    // term in the message itself is stronger evidence than one in a file it
    // happened to carry.
    assert_eq!(hits[0].node_id, in_body);
    assert_eq!(hits[1].node_id, in_attachment);
    assert!(hits[0].score > hits[1].score);
}

#[sqlx::test(migrations = "./migrations")]
async fn match_provenance_distinguishes_every_source(pool: PgPool) {
    let node_id = seed(
        &pool,
        "Quarterly reconciliation",
        "Body mentions defenestration.",
        "warranty.pdf",
        "2",
    )
    .await;
    store_text(&pool, node_id, "2", "Indemnification terms follow.").await;

    let source_for = |term: &'static str| {
        let pool = pool.clone();
        async move {
            let hits = search_email(&pool, Some(term), &EmailSearchFilters::default(), None, 10)
                .await
                .expect("search");
            assert_eq!(hits.len(), 1, "{term} matched {} rows", hits.len());
            hits[0].match_sources.clone()
        }
    };

    assert!(
        source_for("Quarterly")
            .await
            .contains(&"subject".to_owned())
    );
    assert!(
        source_for("defenestration")
            .await
            .contains(&"body".to_owned())
    );
    assert!(
        source_for("warranty.pdf")
            .await
            .contains(&"attachment_filename".to_owned())
    );
    assert!(
        source_for("Indemnification")
            .await
            .contains(&"attachment_content".to_owned())
    );
    assert!(
        source_for("ada@example.test")
            .await
            .contains(&"headers".to_owned())
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_filter_only_search_attributes_nothing(pool: PgPool) {
    let node_id = seed(&pool, "No query", "Body.", "x.pdf", "2").await;
    let hits = search_email(
        &pool,
        None,
        &EmailSearchFilters {
            has_attachment: Some(true),
            ..EmailSearchFilters::default()
        },
        None,
        10,
    )
    .await
    .expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].node_id, node_id);
    // There is no query to attribute a match to, so claiming a source would be
    // an invention.
    assert!(
        hits[0].match_sources.is_empty(),
        "{:?}",
        hits[0].match_sources
    );
    assert!(hits[0].matched_attachment.is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn replacing_text_does_not_accumulate_pages(pool: PgPool) {
    let node_id = seed(&pool, "Rerun", "Body.", "x.pdf", "2").await;
    store_text(&pool, node_id, "2", "first extraction").await;
    store_text(&pool, node_id, "2", "second extraction").await;

    let pages = list_email_attachment_text(&pool, node_id)
        .await
        .expect("list text");
    assert_eq!(pages.len(), 1, "a rerun accumulated a duplicate page");
    assert_eq!(pages[0].content, "second extraction");

    // The stale term must leave the index with the row that carried it.
    let stale = search_email(
        &pool,
        Some("first"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("search");
    assert!(stale.is_empty(), "replaced text stayed searchable");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_failed_attachment_leaves_the_message_completed_and_searchable(pool: PgPool) {
    let node_id = seed(
        &pool,
        "Broken carrier",
        "Body text survives.",
        "bad.pdf",
        "2",
    )
    .await;
    let failure = "extractor exploded".to_owned();
    replace_email_attachment_text(
        &pool,
        node_id,
        "2",
        &[],
        &EmailAttachmentTextOutcome {
            status: EmailExtractionStatus::Failed,
            extractor_name: None,
            extractor_version: Some("1"),
            warnings: std::slice::from_ref(&failure),
        },
    )
    .await
    .expect("record failure");

    let artifact = get_email_attachment_artifact(&pool, node_id, "2")
        .await
        .expect("load artifact")
        .expect("artifact exists");
    assert_eq!(artifact.text_status, EmailExtractionStatus::Failed);
    assert_eq!(artifact.text_warnings, vec![failure]);

    // The message parsed and its body is still findable; one unreadable PDF is
    // not a reason to lose the message.
    let message = strife_db::get_email_message(&pool, node_id)
        .await
        .expect("load message")
        .expect("message exists");
    assert_eq!(message.status, EmailExtractionStatus::Completed);
    let hits = search_email(
        &pool,
        Some("survives"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("search");
    assert_eq!(hits.len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn reprocessing_targets_one_attachment(pool: PgPool) {
    let node_id = seed(&pool, "Targeted", "Body.", "x.pdf", "2").await;
    store_text(&pool, node_id, "2", "text").await;

    let enqueued = enqueue_attachment_reprocessing(
        &pool,
        AttachmentReprocessScope::Part {
            node_id,
            part_path: "2",
        },
        100,
    )
    .await
    .expect("reprocess one");
    assert_eq!(enqueued, 1);

    let artifact = get_email_attachment_artifact(&pool, node_id, "2")
        .await
        .expect("load artifact")
        .expect("artifact exists");
    // Reset to pending, or the handler would skip it as already done.
    assert_eq!(artifact.text_status, EmailExtractionStatus::Pending);
}

#[sqlx::test(migrations = "./migrations")]
async fn reprocessing_targets_failed_missing_and_version_mismatches(pool: PgPool) {
    let failed = seed(&pool, "Failed", "Body.", "a.pdf", "2").await;
    replace_email_attachment_text(
        &pool,
        failed,
        "2",
        &[],
        &EmailAttachmentTextOutcome {
            status: EmailExtractionStatus::Failed,
            extractor_name: None,
            extractor_version: Some("1"),
            warnings: &[],
        },
    )
    .await
    .expect("record failure");

    // Never extracted: the artifact exists but text_status is still pending.
    let missing = seed(&pool, "Missing", "Body.", "b.pdf", "2").await;

    let stale = seed(&pool, "Stale", "Body.", "c.pdf", "2").await;
    store_text(&pool, stale, "2", "extracted by version 1").await;

    assert_eq!(
        enqueue_attachment_reprocessing(&pool, AttachmentReprocessScope::Failed, 100)
            .await
            .expect("failed scope"),
        1
    );
    assert_eq!(
        enqueue_attachment_reprocessing(&pool, AttachmentReprocessScope::Missing, 100)
            .await
            .expect("missing scope"),
        1,
        "only the never-extracted attachment should be missing"
    );
    // Version 2 does not match the stored version 1, so the stale row is picked
    // up; nothing else claims to have been extracted at all.
    assert_eq!(
        enqueue_attachment_reprocessing(
            &pool,
            AttachmentReprocessScope::ExtractorVersion("2"),
            100
        )
        .await
        .expect("version scope"),
        1
    );

    for node_id in [failed, missing, stale] {
        let artifact = get_email_attachment_artifact(&pool, node_id, "2")
            .await
            .expect("load artifact")
            .expect("artifact exists");
        assert_eq!(artifact.text_status, EmailExtractionStatus::Pending);
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn reprocessing_is_bounded_by_its_limit(pool: PgPool) {
    for index in 0..5 {
        seed(&pool, &format!("Bulk {index}"), "Body.", "x.pdf", "2").await;
    }
    let enqueued = enqueue_attachment_reprocessing(&pool, AttachmentReprocessScope::Missing, 2)
        .await
        .expect("bounded reprocess");
    // A ten-year archive cannot afford a reprocess that ignores its limit.
    assert_eq!(enqueued, 2);
}
