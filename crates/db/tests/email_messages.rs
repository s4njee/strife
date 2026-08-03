use chrono::{TimeZone, Utc};
use sqlx::PgPool;
use strife_db::{
    EmailAddressInput, EmailAddressRole, EmailAttachmentInput, EmailExtractionStatus,
    EmailHeaderInput, EmailProjection, JobOrigin, JobResourceClass, JobType, ROOT_NODE_ID,
    UpsertEmailMessage, claim_job, default_resource_class, enqueue_job, get_email_message,
    list_email_addresses, list_email_attachments, list_email_headers, list_email_labels,
    replace_email_projection,
};
use uuid::Uuid;

async fn seed_node(pool: &PgPool) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("message-{node_id}.eml"))
        .execute(pool)
        .await
        .expect("create email node");
    node_id
}

fn message<'a>(node_id: Uuid, subject: &'a str, parser_version: &'a str) -> UpsertEmailMessage<'a> {
    UpsertEmailMessage {
        node_id,
        status: EmailExtractionStatus::Completed,
        parser_name: "test-parser",
        parser_version,
        message_id: Some("<a1@studio.co>"),
        normalized_message_id: Some("a1@studio.co"),
        in_reply_to: None,
        reference_ids: &[],
        subject: Some(subject),
        normalized_subject: Some(subject),
        sent_at: Some(Utc.with_ymd_and_hms(2019, 4, 2, 9, 30, 0).unwrap()),
        received_at: Some(Utc.with_ymd_and_hms(2019, 4, 2, 9, 30, 12).unwrap()),
        body_text: "quarterly numbers attached",
        body_html: Some("<p>quarterly numbers attached</p>"),
        preview_text: "quarterly numbers attached",
        content_hash: Some("deadbeef"),
        provider_thread_id: Some("thread-9001"),
        warnings: &[],
        duration_ms: Some(42),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn projection_stores_addresses_headers_labels_and_attachments(pool: PgPool) {
    let node_id = seed_node(&pool).await;
    let labels = vec!["Inbox".to_owned(), "Work".to_owned()];
    let no_warnings: Vec<String> = Vec::new();
    let stored = replace_email_projection(
        &pool,
        &EmailProjection {
            message: message(node_id, "Q2 numbers", "v1"),
            addresses: &[
                EmailAddressInput {
                    role: EmailAddressRole::From,
                    display_name: Some("A. Reyes"),
                    address: "a.reyes@studio.co",
                },
                EmailAddressInput {
                    role: EmailAddressRole::To,
                    display_name: None,
                    address: "m.okafor@studio.co",
                },
                EmailAddressInput {
                    role: EmailAddressRole::Cc,
                    display_name: Some("L. Park"),
                    address: "l.park@studio.co",
                },
            ],
            // Two `Received` headers: repeats must survive with their order.
            headers: &[
                EmailHeaderInput {
                    name: "Received",
                    value: "by mx1.studio.co",
                },
                EmailHeaderInput {
                    name: "Received",
                    value: "by mx2.studio.co",
                },
                EmailHeaderInput {
                    name: "Subject",
                    value: "Q2 numbers",
                },
            ],
            labels: &labels,
            attachments: &[EmailAttachmentInput {
                part_path: "2",
                filename: Some("numbers.xlsx"),
                media_type: "application/vnd.ms-excel",
                disposition: Some("attachment"),
                content_id: None,
                transfer_encoding: Some("base64"),
                decoded_size: Some(8_192),
                checksum_sha256: Some("0".repeat(64).leak()),
                is_inline: false,
                is_message: false,
                warnings: &no_warnings,
            }],
        },
    )
    .await
    .expect("store projection");

    assert_eq!(stored.status, EmailExtractionStatus::Completed);
    assert_eq!(
        stored.attachment_count, 1,
        "count derived from the manifest"
    );

    let addresses = list_email_addresses(&pool, node_id)
        .await
        .expect("addresses");
    assert_eq!(addresses.len(), 3);
    assert_eq!(addresses[0].role, EmailAddressRole::From);
    assert_eq!(addresses[0].display_name.as_deref(), Some("A. Reyes"));
    assert_eq!(addresses[2].role, EmailAddressRole::Cc);

    let headers = list_email_headers(&pool, node_id).await.expect("headers");
    assert_eq!(headers.len(), 3, "repeated headers must not collapse");
    assert_eq!(headers[0].value, "by mx1.studio.co");
    assert_eq!(headers[1].value, "by mx2.studio.co");
    assert_eq!(
        headers[0].normalized_name, "received",
        "case-insensitive lookup name is stored alongside the original"
    );
    assert_eq!(headers[0].name, "Received", "original casing is preserved");

    assert_eq!(
        list_email_labels(&pool, node_id).await.expect("labels"),
        vec!["Inbox".to_owned(), "Work".to_owned()]
    );

    let attachments = list_email_attachments(&pool, node_id)
        .await
        .expect("attachments");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].filename.as_deref(), Some("numbers.xlsx"));
    assert_eq!(
        attachments[0].extraction_status,
        EmailExtractionStatus::Pending,
        "manifest rows start unextracted"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn reparsing_replaces_every_dependent_row_atomically(pool: PgPool) {
    let node_id = seed_node(&pool).await;
    let first_labels = vec!["Inbox".to_owned()];
    let no_warnings: Vec<String> = Vec::new();
    replace_email_projection(
        &pool,
        &EmailProjection {
            message: message(node_id, "First parse", "v1"),
            addresses: &[EmailAddressInput {
                role: EmailAddressRole::From,
                display_name: None,
                address: "old@studio.co",
            }],
            headers: &[EmailHeaderInput {
                name: "Subject",
                value: "First parse",
            }],
            labels: &first_labels,
            attachments: &[EmailAttachmentInput {
                part_path: "2",
                filename: Some("old.pdf"),
                media_type: "application/pdf",
                disposition: None,
                content_id: None,
                transfer_encoding: None,
                decoded_size: None,
                checksum_sha256: None,
                is_inline: false,
                is_message: false,
                warnings: &no_warnings,
            }],
        },
    )
    .await
    .expect("first parse");

    // A newer parser produces different addresses, headers, labels, and parts.
    let second_labels = vec!["Archive".to_owned()];
    replace_email_projection(
        &pool,
        &EmailProjection {
            message: message(node_id, "Second parse", "v2"),
            addresses: &[EmailAddressInput {
                role: EmailAddressRole::From,
                display_name: None,
                address: "new@studio.co",
            }],
            headers: &[EmailHeaderInput {
                name: "Subject",
                value: "Second parse",
            }],
            labels: &second_labels,
            attachments: &[],
        },
    )
    .await
    .expect("second parse");

    let reloaded = get_email_message(&pool, node_id)
        .await
        .expect("reload")
        .expect("message exists");
    assert_eq!(reloaded.parser_version, "v2");
    assert_eq!(reloaded.subject.as_deref(), Some("Second parse"));
    assert_eq!(reloaded.attachment_count, 0);

    let addresses = list_email_addresses(&pool, node_id)
        .await
        .expect("addresses");
    assert_eq!(addresses.len(), 1, "stale addresses were not replaced");
    assert_eq!(addresses[0].address, "new@studio.co");
    assert_eq!(
        list_email_labels(&pool, node_id).await.expect("labels"),
        vec!["Archive".to_owned()],
        "stale labels were not replaced"
    );
    assert!(
        list_email_attachments(&pool, node_id)
            .await
            .expect("attachments")
            .is_empty(),
        "a message cannot keep attachments from an older parser version"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn deleting_a_node_removes_its_parsed_projection(pool: PgPool) {
    let node_id = seed_node(&pool).await;
    let labels = vec!["Inbox".to_owned()];
    let no_warnings: Vec<String> = Vec::new();
    replace_email_projection(
        &pool,
        &EmailProjection {
            message: message(node_id, "Cascade", "v1"),
            addresses: &[EmailAddressInput {
                role: EmailAddressRole::From,
                display_name: None,
                address: "a@studio.co",
            }],
            headers: &[EmailHeaderInput {
                name: "Subject",
                value: "Cascade",
            }],
            labels: &labels,
            attachments: &[EmailAttachmentInput {
                part_path: "2",
                filename: None,
                media_type: "text/plain",
                disposition: None,
                content_id: None,
                transfer_encoding: None,
                decoded_size: None,
                checksum_sha256: None,
                is_inline: true,
                is_message: false,
                warnings: &no_warnings,
            }],
        },
    )
    .await
    .expect("store projection");

    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("delete node");

    assert!(
        get_email_message(&pool, node_id)
            .await
            .expect("reload")
            .is_none()
    );
    assert!(
        list_email_addresses(&pool, node_id)
            .await
            .expect("addresses")
            .is_empty()
    );
    assert!(
        list_email_headers(&pool, node_id)
            .await
            .expect("headers")
            .is_empty()
    );
    assert!(
        list_email_labels(&pool, node_id)
            .await
            .expect("labels")
            .is_empty()
    );
    assert!(
        list_email_attachments(&pool, node_id)
            .await
            .expect("attachments")
            .is_empty()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn schema_constraints_reject_impossible_rows(pool: PgPool) {
    let node_id = seed_node(&pool).await;
    let labels: Vec<String> = Vec::new();
    replace_email_projection(
        &pool,
        &EmailProjection {
            message: message(node_id, "Constraints", "v1"),
            addresses: &[],
            headers: &[],
            labels: &labels,
            attachments: &[],
        },
    )
    .await
    .expect("store projection");

    let negative_duration =
        sqlx::query("UPDATE email_messages SET duration_ms = -1 WHERE node_id = $1")
            .bind(node_id)
            .execute(&pool)
            .await;
    assert!(negative_duration.is_err(), "negative duration was accepted");

    let negative_count =
        sqlx::query("UPDATE email_messages SET attachment_count = -1 WHERE node_id = $1")
            .bind(node_id)
            .execute(&pool)
            .await;
    assert!(
        negative_count.is_err(),
        "negative attachment count accepted"
    );

    let negative_size = sqlx::query(
        r"
        INSERT INTO email_attachments (node_id, part_path, position, media_type, decoded_size)
        VALUES ($1, '2', 0, 'text/plain', -1)
        ",
    )
    .bind(node_id)
    .execute(&pool)
    .await;
    assert!(negative_size.is_err(), "negative decoded size was accepted");

    // Two parts of one message cannot claim the same MIME part path.
    sqlx::query(
        "INSERT INTO email_attachments (node_id, part_path, position, media_type) VALUES ($1, '2', 0, 'text/plain')",
    )
    .bind(node_id)
    .execute(&pool)
    .await
    .expect("first part");
    let duplicate_part = sqlx::query(
        "INSERT INTO email_attachments (node_id, part_path, position, media_type) VALUES ($1, '2', 1, 'text/plain')",
    )
    .bind(node_id)
    .execute(&pool)
    .await;
    assert!(duplicate_part.is_err(), "duplicate part path was accepted");

    // Grouping identifiers are hints: several nodes may legitimately share one.
    let sibling = seed_node(&pool).await;
    replace_email_projection(
        &pool,
        &EmailProjection {
            message: message(sibling, "Sibling", "v1"),
            addresses: &[],
            headers: &[],
            labels: &labels,
            attachments: &[],
        },
    )
    .await
    .expect("store sibling");
    let group = Uuid::new_v4();
    for target in [node_id, sibling] {
        sqlx::query(
            "UPDATE email_messages SET thread_group_id = $2, duplicate_group_id = $2 WHERE node_id = $1",
        )
        .bind(target)
        .bind(group)
        .execute(&pool)
        .await
        .expect("group identifiers must not be unique");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn email_jobs_are_unique_per_node_and_claimable(pool: PgPool) {
    let node_id = seed_node(&pool).await;
    assert_eq!(
        default_resource_class(JobType::EmailExtraction),
        JobResourceClass::HeavyCpu,
        "email starts under the shared heavy permit per ADR 0009"
    );

    let first = enqueue_job(&pool, JobType::EmailExtraction, node_id, 0)
        .await
        .expect("enqueue email job");
    assert!(first.is_some());
    assert_eq!(first.as_ref().expect("job").origin, JobOrigin::Foreground);

    // The existing per-type active-job uniqueness rule must cover email too.
    let duplicate = enqueue_job(&pool, JobType::EmailExtraction, node_id, 0)
        .await
        .expect("second enqueue");
    assert!(duplicate.is_none(), "a second active email job was created");

    let claimed = claim_job(
        &pool,
        JobType::EmailExtraction,
        "email-fixture",
        chrono::Duration::minutes(1),
    )
    .await
    .expect("claim email job")
    .expect("claimable email job");
    assert_eq!(claimed.target_node_id, node_id);
    assert_eq!(claimed.job_type, JobType::EmailExtraction);
}
