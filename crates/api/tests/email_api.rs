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
