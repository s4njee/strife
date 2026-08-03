use chrono::{DateTime, Duration, TimeZone, Utc};
use sqlx::PgPool;
use strife_db::{
    EmailAddressInput, EmailAddressRole, EmailAttachmentInput, EmailExtractionStatus,
    EmailProjection, EmailSearchCursor, EmailSearchFilters, ROOT_NODE_ID, UpsertEmailMessage,
    backfill_email_search_vectors, count_email_messages_without_search_vector,
    email_correspondent_facets, email_label_facets, email_year_facets, replace_email_projection,
    search_email,
};
use uuid::Uuid;

struct Seed<'a> {
    subject: &'a str,
    body: &'a str,
    from: &'a str,
    to: &'a str,
    labels: Vec<String>,
    attachment: Option<&'a str>,
    sent_at: Option<DateTime<Utc>>,
}

impl Default for Seed<'_> {
    fn default() -> Self {
        Self {
            subject: "Subject",
            body: "Body",
            from: "ada@example.test",
            to: "bob@example.test",
            labels: Vec::new(),
            attachment: None,
            sent_at: Some(Utc.with_ymd_and_hms(2019, 5, 1, 12, 0, 0).unwrap()),
        }
    }
}

async fn seed(pool: &PgPool, input: Seed<'_>) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("search-{node_id}.eml"))
        .execute(pool)
        .await
        .expect("create node");
    let attachments: Vec<EmailAttachmentInput<'_>> = input
        .attachment
        .map(|filename| {
            vec![EmailAttachmentInput {
                part_path: "2",
                filename: Some(filename),
                media_type: "application/pdf",
                disposition: None,
                content_id: None,
                transfer_encoding: None,
                decoded_size: None,
                checksum_sha256: None,
                is_inline: false,
                is_message: false,
                warnings: &[],
            }]
        })
        .unwrap_or_default();
    replace_email_projection(
        pool,
        &EmailProjection {
            message: UpsertEmailMessage {
                node_id,
                status: EmailExtractionStatus::Completed,
                parser_name: "mail-parser",
                parser_version: "0.11.5",
                message_id: None,
                normalized_message_id: None,
                in_reply_to: None,
                reference_ids: &[],
                subject: Some(input.subject),
                normalized_subject: Some(input.subject),
                sent_at: input.sent_at,
                received_at: None,
                body_text: input.body,
                body_html: None,
                preview_text: input.body,
                content_hash: None,
                provider_thread_id: None,
                warnings: &[],
                duration_ms: None,
            },
            addresses: &[
                EmailAddressInput {
                    role: EmailAddressRole::From,
                    display_name: None,
                    address: input.from,
                },
                EmailAddressInput {
                    role: EmailAddressRole::To,
                    display_name: None,
                    address: input.to,
                },
            ],
            headers: &[],
            labels: &input.labels,
            attachments: &attachments,
        },
    )
    .await
    .expect("seed projection");
    node_id
}

#[sqlx::test(migrations = "./migrations")]
async fn a_subject_match_outranks_the_same_term_in_the_body(pool: PgPool) {
    let subject_hit = seed(
        &pool,
        Seed {
            subject: "Quarterly budget review",
            body: "Nothing relevant here at all.",
            ..Seed::default()
        },
    )
    .await;
    let body_hit = seed(
        &pool,
        Seed {
            subject: "Unrelated heading",
            body: "We should discuss the budget when convenient.",
            ..Seed::default()
        },
    )
    .await;

    let results = search_email(
        &pool,
        Some("budget"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("search");
    assert_eq!(results.len(), 2, "both messages should match");
    assert_eq!(
        results[0].node_id, subject_hit,
        "weight A subject must outrank a weight C body match"
    );
    assert_eq!(results[1].node_id, body_hit);
    assert!(results[0].score > results[1].score);
}

#[sqlx::test(migrations = "./migrations")]
async fn stemming_applies_to_prose_but_not_to_addresses(pool: PgPool) {
    let node_id = seed(
        &pool,
        Seed {
            subject: "Meeting notes",
            body: "We reviewed the meetings from last week.",
            from: "a.reyes@example.test",
            ..Seed::default()
        },
    )
    .await;

    // English stemming: the singular query matches the plural in the body.
    let stemmed = search_email(
        &pool,
        Some("meetings"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("stemmed search");
    assert_eq!(stemmed.len(), 1, "prose must be stemmed");
    assert_eq!(stemmed[0].node_id, node_id);

    // The address survives verbatim; stemming would mangle it beyond matching.
    let exact = search_email(
        &pool,
        Some("a.reyes@example.test"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("address search");
    assert_eq!(exact.len(), 1, "an address token was mangled by stemming");
    assert_eq!(exact[0].node_id, node_id);
}

#[sqlx::test(migrations = "./migrations")]
async fn labels_and_attachment_filenames_are_searchable(pool: PgPool) {
    let node_id = seed(
        &pool,
        Seed {
            subject: "Nothing special",
            body: "Nothing special either.",
            labels: vec!["Receipts".to_owned()],
            attachment: Some("invoice-2019.pdf"),
            ..Seed::default()
        },
    )
    .await;

    for term in ["Receipts", "invoice-2019.pdf"] {
        let results = search_email(&pool, Some(term), &EmailSearchFilters::default(), None, 10)
            .await
            .expect("search");
        assert_eq!(results.len(), 1, "{term} did not match");
        assert_eq!(results[0].node_id, node_id);
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn dependent_changes_keep_the_index_current(pool: PgPool) {
    let node_id = seed(
        &pool,
        Seed {
            subject: "Static subject",
            body: "Static body.",
            labels: vec!["Before".to_owned()],
            ..Seed::default()
        },
    )
    .await;
    assert_eq!(
        search_email(
            &pool,
            Some("Before"),
            &EmailSearchFilters::default(),
            None,
            10
        )
        .await
        .expect("search")
        .len(),
        1
    );

    // Replacing the projection changes the labels; the vector must follow.
    let labels = vec!["After".to_owned()];
    replace_email_projection(
        &pool,
        &EmailProjection {
            message: UpsertEmailMessage {
                node_id,
                status: EmailExtractionStatus::Completed,
                parser_name: "mail-parser",
                parser_version: "0.11.5",
                message_id: None,
                normalized_message_id: None,
                in_reply_to: None,
                reference_ids: &[],
                subject: Some("Static subject"),
                normalized_subject: Some("Static subject"),
                sent_at: None,
                received_at: None,
                body_text: "Static body.",
                body_html: None,
                preview_text: "Static body.",
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
    .expect("replace projection");

    assert!(
        search_email(
            &pool,
            Some("Before"),
            &EmailSearchFilters::default(),
            None,
            10
        )
        .await
        .expect("search")
        .is_empty(),
        "a stale label still matches"
    );
    assert_eq!(
        search_email(
            &pool,
            Some("After"),
            &EmailSearchFilters::default(),
            None,
            10
        )
        .await
        .expect("search")
        .len(),
        1,
        "a new label is not searchable"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn existing_rows_are_indexed_by_a_bounded_backfill(pool: PgPool) {
    seed(&pool, Seed::default()).await;
    seed(&pool, Seed::default()).await;
    // Simulate rows written before the search migration added the column. The
    // trigger is disabled for the simulation only; in production those rows are
    // NULL because the column did not exist when they were written.
    for statement in [
        "ALTER TABLE email_messages DISABLE TRIGGER email_messages_search_vector",
        "UPDATE email_messages SET search_vector = NULL",
        "ALTER TABLE email_messages ENABLE TRIGGER email_messages_search_vector",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(&pool)
            .await
            .expect("simulate pre-migration rows");
    }
    assert_eq!(
        count_email_messages_without_search_vector(&pool)
            .await
            .expect("count"),
        2
    );

    let updated = backfill_email_search_vectors(&pool, 1)
        .await
        .expect("bounded backfill");
    assert_eq!(updated, 1, "the batch limit was not applied");
    assert_eq!(
        count_email_messages_without_search_vector(&pool)
            .await
            .expect("count"),
        1
    );

    backfill_email_search_vectors(&pool, 100)
        .await
        .expect("finish backfill");
    assert_eq!(
        count_email_messages_without_search_vector(&pool)
            .await
            .expect("count"),
        0,
        "indexing only future inserts is insufficient"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn structured_filters_narrow_independently(pool: PgPool) {
    let target = seed(
        &pool,
        Seed {
            subject: "Filtered",
            body: "Filtered body.",
            from: "sender@example.test",
            to: "receiver@example.test",
            labels: vec!["Work".to_owned()],
            attachment: Some("doc.pdf"),
            sent_at: Some(Utc.with_ymd_and_hms(2020, 6, 1, 0, 0, 0).unwrap()),
        },
    )
    .await;
    seed(
        &pool,
        Seed {
            subject: "Other",
            body: "Other body.",
            from: "someone@example.test",
            to: "nobody@example.test",
            sent_at: Some(Utc.with_ymd_and_hms(2015, 1, 1, 0, 0, 0).unwrap()),
            ..Seed::default()
        },
    )
    .await;

    let cases: Vec<EmailSearchFilters> = vec![
        EmailSearchFilters {
            from: vec!["sender@example.test".to_owned()],
            ..EmailSearchFilters::default()
        },
        EmailSearchFilters {
            participant: vec!["receiver@example.test".to_owned()],
            ..EmailSearchFilters::default()
        },
        EmailSearchFilters {
            labels: vec!["Work".to_owned()],
            ..EmailSearchFilters::default()
        },
        EmailSearchFilters {
            has_attachment: Some(true),
            ..EmailSearchFilters::default()
        },
        EmailSearchFilters {
            after: Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()),
            ..EmailSearchFilters::default()
        },
        EmailSearchFilters {
            before: Some(Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap()),
            after: Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()),
            ..EmailSearchFilters::default()
        },
    ];
    for (index, filters) in cases.iter().enumerate() {
        let results = search_email(&pool, None, filters, None, 10)
            .await
            .expect("filter-only search");
        assert_eq!(results.len(), 1, "filter case {index} matched wrongly");
        assert_eq!(results[0].node_id, target, "filter case {index}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn trashed_messages_are_excluded_unless_requested(pool: PgPool) {
    let node_id = seed(
        &pool,
        Seed {
            subject: "Discarded",
            body: "Discarded body.",
            ..Seed::default()
        },
    )
    .await;
    strife_db::trash_node(&pool, node_id).await.expect("trash");

    assert!(
        search_email(
            &pool,
            Some("Discarded"),
            &EmailSearchFilters::default(),
            None,
            10
        )
        .await
        .expect("search")
        .is_empty(),
        "trashed messages must be excluded by default"
    );
    assert_eq!(
        search_email(
            &pool,
            Some("Discarded"),
            &EmailSearchFilters {
                include_trashed: true,
                ..EmailSearchFilters::default()
            },
            None,
            10
        )
        .await
        .expect("search")
        .len(),
        1
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn duplicates_collapse_by_default_and_expand_on_request(pool: PgPool) {
    let first = seed(
        &pool,
        Seed {
            subject: "Duplicated message",
            body: "Duplicated body.",
            ..Seed::default()
        },
    )
    .await;
    let second = seed(
        &pool,
        Seed {
            subject: "Duplicated message",
            body: "Duplicated body.",
            ..Seed::default()
        },
    )
    .await;
    let group = Uuid::new_v4();
    for node_id in [first, second] {
        sqlx::query("UPDATE email_messages SET duplicate_group_id = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(group)
            .execute(&pool)
            .await
            .expect("assign duplicate group");
    }

    let collapsed = search_email(
        &pool,
        Some("Duplicated"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("collapsed search");
    assert_eq!(collapsed.len(), 1, "duplicates did not collapse");
    assert_eq!(collapsed[0].duplicate_count, 2, "count must reflect both");

    let expanded = search_email(
        &pool,
        Some("Duplicated"),
        &EmailSearchFilters {
            include_duplicates: true,
            ..EmailSearchFilters::default()
        },
        None,
        10,
    )
    .await
    .expect("expanded search");
    assert_eq!(expanded.len(), 2, "every original must remain reachable");
}

#[sqlx::test(migrations = "./migrations")]
async fn cursor_paging_is_stable_across_equal_scores(pool: PgPool) {
    // Identical content means identical scores, which is exactly when an
    // offset-based pager would repeat or skip rows.
    for index in 0..5 {
        seed(
            &pool,
            Seed {
                subject: "Identical subject",
                body: "Identical body.",
                sent_at: Some(
                    Utc.with_ymd_and_hms(2019, 5, 1, 12, 0, 0).unwrap() + Duration::seconds(index),
                ),
                ..Seed::default()
            },
        )
        .await;
    }

    let mut seen: Vec<Uuid> = Vec::new();
    let mut cursor: Option<EmailSearchCursor> = None;
    for _ in 0..5 {
        let page = search_email(
            &pool,
            Some("Identical"),
            &EmailSearchFilters::default(),
            cursor,
            2,
        )
        .await
        .expect("page");
        if page.is_empty() {
            break;
        }
        for hit in &page {
            seen.push(hit.node_id);
        }
        let last = page.last().expect("non-empty page");
        cursor = Some(EmailSearchCursor {
            score: last.score,
            sent_at: last.sent_at,
            node_id: last.node_id,
        });
    }

    let mut deduped = seen.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(seen.len(), 5, "paging skipped or stopped early");
    assert_eq!(deduped.len(), 5, "paging returned a row twice");
}

#[sqlx::test(migrations = "./migrations")]
async fn snippets_mark_matched_terms_without_emitting_markup(pool: PgPool) {
    seed(
        &pool,
        Seed {
            subject: "Snippet source",
            body: "The quarterly reconciliation figures are attached for review.",
            ..Seed::default()
        },
    )
    .await;
    let results = search_email(
        &pool,
        Some("reconciliation"),
        &EmailSearchFilters::default(),
        None,
        10,
    )
    .await
    .expect("search");
    let snippet = &results[0].snippet;
    assert!(
        snippet.contains("[[reconciliation]]"),
        "matched term was not marked: {snippet}"
    );
    // Markers are parsed by the frontend into text nodes; emitting HTML here
    // would invite injection through archived message content.
    assert!(!snippet.contains('<'), "snippet emitted markup: {snippet}");
}

#[sqlx::test(migrations = "./migrations")]
async fn facets_are_bounded_and_scoped_to_active_messages(pool: PgPool) {
    seed(
        &pool,
        Seed {
            labels: vec!["Work".to_owned(), "Receipts".to_owned()],
            from: "ada@example.test",
            sent_at: Some(Utc.with_ymd_and_hms(2019, 5, 1, 12, 0, 0).unwrap()),
            ..Seed::default()
        },
    )
    .await;
    let trashed = seed(
        &pool,
        Seed {
            labels: vec!["Work".to_owned()],
            from: "ghost@example.test",
            ..Seed::default()
        },
    )
    .await;
    strife_db::trash_node(&pool, trashed).await.expect("trash");

    let labels = email_label_facets(&pool, 10).await.expect("label facets");
    let work = labels
        .iter()
        .find(|facet| facet.value == "Work")
        .expect("Work facet");
    assert_eq!(work.count, 1, "a trashed message was counted");

    let correspondents = email_correspondent_facets(&pool, 10)
        .await
        .expect("correspondent facets");
    assert!(correspondents.iter().any(|f| f.value == "ada@example.test"));
    assert!(
        !correspondents
            .iter()
            .any(|f| f.value == "ghost@example.test"),
        "a trashed correspondent leaked into the facets"
    );

    let bounded = email_label_facets(&pool, 1).await.expect("bounded facets");
    assert_eq!(bounded.len(), 1, "facet limit was not applied");

    let years = email_year_facets(&pool).await.expect("year facets");
    assert!(years.iter().any(|facet| facet.value == "2019"));
}
