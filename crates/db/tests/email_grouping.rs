//! Thread and duplicate grouping as persisted and queried.
//!
//! The resolver's own edge cases are unit-tested next to it; this covers the
//! part that only shows up against a database — that grouping is written by the
//! projection, survives a reparse, and drives the thread and duplicate filters.

use chrono::{TimeZone, Utc};
use sqlx::PgPool;
use strife_db::{
    EmailAddressInput, EmailAddressRole, EmailDuplicateReason, EmailExtractionStatus,
    EmailProjection, EmailSearchFilters, EmailThreadReason, ROOT_NODE_ID, UpsertEmailMessage,
    get_email_message, replace_email_projection, search_email,
};
use uuid::Uuid;

#[allow(clippy::struct_field_names)]
struct Message<'a> {
    subject: &'a str,
    body: &'a str,
    message_id: Option<&'a str>,
    in_reply_to: Option<&'a str>,
    references: &'a [String],
    provider_thread_id: Option<&'a str>,
    content_hash: Option<&'a str>,
}

impl Default for Message<'_> {
    fn default() -> Self {
        Self {
            subject: "Quarterly review",
            body: "Body text.",
            message_id: None,
            in_reply_to: None,
            references: &[],
            provider_thread_id: None,
            content_hash: None,
        }
    }
}

async fn seed(pool: &PgPool, message: &Message<'_>) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("group-{node_id}.eml"))
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
                message_id: message.message_id,
                normalized_message_id: message.message_id,
                in_reply_to: message.in_reply_to,
                reference_ids: message.references,
                subject: Some(message.subject),
                normalized_subject: Some(message.subject),
                sent_at: Some(Utc.with_ymd_and_hms(2019, 7, 4, 12, 0, 0).unwrap()),
                received_at: None,
                body_text: message.body,
                body_html: None,
                preview_text: message.body,
                content_hash: message.content_hash,
                provider_thread_id: message.provider_thread_id,
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
            attachments: &[],
        },
    )
    .await
    .expect("seed projection");
    node_id
}

#[sqlx::test(migrations = "./migrations")]
async fn a_reply_chain_is_grouped_and_navigable_by_thread(pool: PgPool) {
    let root_refs = vec!["root@example.test".to_owned()];
    let root = seed(
        &pool,
        &Message {
            message_id: Some("root@example.test"),
            ..Message::default()
        },
    )
    .await;
    let reply = seed(
        &pool,
        &Message {
            subject: "Re: Quarterly review",
            message_id: Some("reply@example.test"),
            in_reply_to: Some("root@example.test"),
            references: &root_refs,
            ..Message::default()
        },
    )
    .await;

    let root_record = get_email_message(&pool, root)
        .await
        .expect("load root")
        .expect("root exists");
    let reply_record = get_email_message(&pool, reply)
        .await
        .expect("load reply")
        .expect("reply exists");
    assert_eq!(root_record.thread_group_id, reply_record.thread_group_id);
    assert_eq!(root_record.thread_reason, EmailThreadReason::MessageId);
    assert_eq!(reply_record.thread_reason, EmailThreadReason::References);

    // The thread id is what the UI navigates by, so it has to select both.
    let thread = search_email(
        &pool,
        None,
        &EmailSearchFilters {
            thread_group_id: root_record.thread_group_id,
            ..EmailSearchFilters::default()
        },
        None,
        10,
    )
    .await
    .expect("thread search");
    assert_eq!(thread.len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn duplicates_collapse_but_every_original_remains_reachable(pool: PgPool) {
    let mut nodes = Vec::new();
    for _ in 0..3 {
        nodes.push(
            seed(
                &pool,
                &Message {
                    subject: "Receipt",
                    body: "Thank you for your order.",
                    message_id: Some("receipt@example.test"),
                    ..Message::default()
                },
            )
            .await,
        );
    }

    let collapsed = search_email(
        &pool,
        Some("order"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("collapsed search");
    assert_eq!(collapsed.len(), 1, "duplicates were not collapsed");

    // Nothing was deleted: every copy is still a node, and revealing them shows
    // all three so a user can open whichever original they need.
    let expanded = search_email(
        &pool,
        Some("order"),
        &EmailSearchFilters {
            include_duplicates: true,
            ..EmailSearchFilters::default()
        },
        None,
        10,
    )
    .await
    .expect("expanded search");
    assert_eq!(expanded.len(), 3);
    for node_id in nodes {
        assert!(expanded.iter().any(|hit| hit.node_id == node_id));
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_message_without_an_id_falls_back_to_its_content_hash(pool: PgPool) {
    let first = seed(
        &pool,
        &Message {
            subject: "No identity",
            content_hash: Some("deadbeef"),
            ..Message::default()
        },
    )
    .await;
    let second = seed(
        &pool,
        &Message {
            subject: "No identity",
            content_hash: Some("deadbeef"),
            ..Message::default()
        },
    )
    .await;

    let a = get_email_message(&pool, first)
        .await
        .expect("load")
        .expect("exists");
    let b = get_email_message(&pool, second)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(a.duplicate_group_id, b.duplicate_group_id);
    assert_eq!(a.duplicate_reason, EmailDuplicateReason::ContentHash);
    // Subject is the only threading evidence left, and it is recorded as such
    // so a weak grouping can be told apart from a strong one.
    assert_eq!(a.thread_reason, EmailThreadReason::Subject);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_provider_thread_id_groups_messages_the_headers_would_separate(pool: PgPool) {
    let first = seed(
        &pool,
        &Message {
            subject: "Moved thread",
            message_id: Some("a@example.test"),
            provider_thread_id: Some("987654321"),
            ..Message::default()
        },
    )
    .await;
    let second = seed(
        &pool,
        &Message {
            subject: "Completely different subject",
            message_id: Some("b@example.test"),
            provider_thread_id: Some("987654321"),
            ..Message::default()
        },
    )
    .await;

    let a = get_email_message(&pool, first)
        .await
        .expect("load")
        .expect("exists");
    let b = get_email_message(&pool, second)
        .await
        .expect("load")
        .expect("exists");
    // Gmail knows about conversations the RFC headers never recorded.
    assert_eq!(a.thread_group_id, b.thread_group_id);
    assert_eq!(a.thread_reason, EmailThreadReason::Provider);
    assert!(!a.thread_conflict, "no References to disagree with");
}

#[sqlx::test(migrations = "./migrations")]
async fn reparsing_a_message_keeps_its_grouping_stable(pool: PgPool) {
    let refs = vec!["root@example.test".to_owned()];
    let message = Message {
        message_id: Some("stable@example.test"),
        references: &refs,
        ..Message::default()
    };
    let node_id = seed(&pool, &message).await;
    let before = get_email_message(&pool, node_id)
        .await
        .expect("load")
        .expect("exists");

    // A reparse must land on the same groups, or a backfill would scatter a
    // thread across new ids every time it ran.
    replace_email_projection(
        &pool,
        &EmailProjection {
            message: UpsertEmailMessage {
                node_id,
                status: EmailExtractionStatus::Completed,
                parser_name: "test",
                parser_version: "2",
                message_id: message.message_id,
                normalized_message_id: message.message_id,
                in_reply_to: None,
                reference_ids: &refs,
                subject: Some(message.subject),
                normalized_subject: Some(message.subject),
                sent_at: None,
                received_at: None,
                body_text: message.body,
                body_html: None,
                preview_text: message.body,
                content_hash: None,
                provider_thread_id: None,
                warnings: &[],
                duration_ms: None,
            },
            addresses: &[],
            headers: &[],
            labels: &[],
            attachments: &[],
        },
    )
    .await
    .expect("reparse");

    let after = get_email_message(&pool, node_id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(before.thread_group_id, after.thread_group_id);
    assert_eq!(before.duplicate_group_id, after.duplicate_group_id);
}

#[sqlx::test(migrations = "./migrations")]
async fn labels_are_preserved_as_imported_facts_including_unicode(pool: PgPool) {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind("labels.eml")
        .execute(&pool)
        .await
        .expect("create node");
    let labels = vec![
        "Reçus".to_owned(),
        "仕事".to_owned(),
        "Travel/2019".to_owned(),
    ];
    replace_email_projection(
        &pool,
        &EmailProjection {
            message: UpsertEmailMessage {
                node_id,
                status: EmailExtractionStatus::Completed,
                parser_name: "test",
                parser_version: "1",
                message_id: Some("labels@example.test"),
                normalized_message_id: Some("labels@example.test"),
                in_reply_to: None,
                reference_ids: &[],
                subject: Some("Labelled"),
                normalized_subject: Some("Labelled"),
                sent_at: None,
                received_at: None,
                body_text: "Body.",
                body_html: None,
                preview_text: "Body.",
                content_hash: None,
                provider_thread_id: None,
                warnings: &[],
                duration_ms: None,
            },
            addresses: &[],
            headers: &[],
            labels: &labels,
            attachments: &[],
        },
    )
    .await
    .expect("seed projection");

    let stored = strife_db::list_email_labels(&pool, node_id)
        .await
        .expect("list labels");
    // Stored verbatim: a Gmail label is a fact about the export, not something
    // Strife normalizes or claims to keep in sync.
    assert_eq!(stored.len(), 3);
    for label in &labels {
        assert!(stored.contains(label), "{label} was altered or dropped");
    }

    let hits = search_email(
        &pool,
        None,
        &EmailSearchFilters {
            labels: vec!["仕事".to_owned()],
            ..EmailSearchFilters::default()
        },
        None,
        10,
    )
    .await
    .expect("label filter");
    assert_eq!(hits.len(), 1);
}
