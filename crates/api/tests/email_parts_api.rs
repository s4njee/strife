//! Attachment streaming: authorization, header safety, and unsafe types.
//!
//! An attachment is sender-controlled data reached by a URL, so the cases that
//! matter are the ones where a sender chose the filename, the MIME type, or the
//! part path with an attack in mind.

use std::sync::Arc;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::{TimeZone, Utc};
use sqlx::PgPool;
use strife_db::{
    EmailAddressInput, EmailAddressRole, EmailArtifactState, EmailAttachmentInput,
    EmailExtractionStatus, EmailProjection, ROOT_NODE_ID, UpsertEmailAttachmentArtifact,
    UpsertEmailMessage, replace_email_projection, upsert_email_attachment_artifact,
};
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use tower::ServiceExt;
use uuid::Uuid;

struct Harness {
    storage: Arc<dyn StorageBackend>,
    root: std::path::PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn harness() -> Harness {
    let root = std::env::temp_dir().join(format!("strife-parts-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let storage: Arc<dyn StorageBackend> = Arc::new(
        LocalFsBackend::new(root.clone())
            .await
            .expect("create backend"),
    );
    Harness { storage, root }
}

/// Seeds a message with one attachment whose bytes are in managed storage.
async fn seed(
    pool: &PgPool,
    harness: &Harness,
    part_path: &str,
    filename: Option<&str>,
    media_type: &str,
    bytes: &[u8],
) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("parts-{node_id}.eml"))
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
                subject: Some("Carrier"),
                normalized_subject: Some("Carrier"),
                sent_at: Some(Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap()),
                received_at: None,
                body_text: "Body.",
                body_html: None,
                preview_text: "Body.",
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
                filename,
                media_type,
                disposition: Some("attachment"),
                content_id: None,
                transfer_encoding: None,
                decoded_size: i64::try_from(bytes.len()).ok(),
                checksum_sha256: None,
                is_inline: false,
                is_message: false,
                warnings: &[],
            }],
        },
    )
    .await
    .expect("seed projection");

    let object_id = strife_db::email_attachment_artifact_id(node_id, part_path);
    harness
        .storage
        .put_stream(
            StorageKey::artifact(object_id),
            Box::pin(std::io::Cursor::new(bytes.to_vec())),
        )
        .await
        .expect("write artifact bytes");
    upsert_email_attachment_artifact(
        pool,
        &UpsertEmailAttachmentArtifact {
            node_id,
            part_path,
            state: EmailArtifactState::Ready,
            storage_key: Some(&object_id.to_string()),
            media_type,
            byte_size: i64::try_from(bytes.len()).expect("size"),
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

async fn get(
    app: axum::Router,
    uri: &str,
    range: Option<&str>,
) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    let mut request = Request::get(uri);
    if let Some(range) = range {
        request = request.header(header::RANGE, range);
    }
    let response = app
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let bytes = to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("read body")
        .to_vec();
    (status, headers, bytes)
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_attachment_streams_with_range_support(pool: PgPool) {
    let harness = harness().await;
    let node_id = seed(
        &pool,
        &harness,
        "2",
        Some("report.pdf"),
        "application/pdf",
        b"0123456789",
    )
    .await;
    let app = strife_api::email_parts::router(pool, harness.storage.clone());

    let (status, headers, body) = get(
        app.clone(),
        &format!("/api/email/messages/{node_id}/parts/2"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"0123456789");
    assert_eq!(header_value(&headers, "accept-ranges"), Some("bytes"));
    assert_eq!(
        header_value(&headers, "x-content-type-options"),
        Some("nosniff")
    );

    let (partial, headers, body) = get(
        app,
        &format!("/api/email/messages/{node_id}/parts/2"),
        Some("bytes=2-5"),
    )
    .await;
    assert_eq!(partial, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, b"2345");
    assert_eq!(
        header_value(&headers, "content-range"),
        Some("bytes 2-5/10")
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_traversal_filename_cannot_escape_the_content_disposition_header(pool: PgPool) {
    let harness = harness().await;
    let node_id = seed(
        &pool,
        &harness,
        "2",
        Some("../../etc/passwd\"; drop=\"1"),
        "application/pdf",
        b"x",
    )
    .await;
    let app = strife_api::email_parts::router(pool, harness.storage.clone());
    let (status, headers, _) =
        get(app, &format!("/api/email/messages/{node_id}/parts/2"), None).await;
    assert_eq!(status, StatusCode::OK);
    let disposition = header_value(&headers, "content-disposition").expect("disposition");
    assert!(disposition.starts_with("attachment;"), "{disposition}");
    // The quote that would close the filename parameter is neutralized, so the
    // sender cannot append a parameter of their own — `drop` survives only as
    // literal text inside the quoted name.
    assert!(!disposition.contains("\"; drop="), "{disposition}");
    // Path separators are gone, so the browser saves a file rather than a path.
    assert!(!disposition.contains('/'), "{disposition}");
    assert!(!disposition.contains(".."), "{disposition}");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_header_injection_filename_cannot_add_headers(pool: PgPool) {
    let harness = harness().await;
    let node_id = seed(
        &pool,
        &harness,
        "2",
        Some("evil\r\nSet-Cookie: session=stolen"),
        "application/pdf",
        b"x",
    )
    .await;
    let app = strife_api::email_parts::router(pool, harness.storage.clone());
    let (status, headers, _) =
        get(app, &format!("/api/email/messages/{node_id}/parts/2"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        header_value(&headers, "set-cookie").is_none(),
        "{headers:?}"
    );
    let disposition = header_value(&headers, "content-disposition").expect("disposition");
    assert!(!disposition.contains('\r') && !disposition.contains('\n'));
}

#[sqlx::test(migrations = "../db/migrations")]
async fn unsafe_types_download_rather_than_render_inline(pool: PgPool) {
    let harness = harness().await;
    for media_type in [
        "image/svg+xml",
        "text/html",
        "application/xhtml+xml",
        "application/x-msdownload",
        "application/octet-stream",
    ] {
        let node_id = seed(
            &pool,
            &harness,
            "2",
            Some("payload"),
            media_type,
            b"<svg onload=\"steal()\"/>",
        )
        .await;
        let app = strife_api::email_parts::router(pool.clone(), harness.storage.clone());
        // Even when inline is explicitly requested.
        let (status, headers, _) = get(
            app,
            &format!("/api/email/messages/{node_id}/parts/2?inline=true"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            header_value(&headers, "content-disposition")
                .expect("disposition")
                .starts_with("attachment;"),
            "{media_type} was served inline"
        );
    }
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_allowlisted_type_may_render_inline_under_a_restrictive_policy(pool: PgPool) {
    let harness = harness().await;
    let node_id = seed(
        &pool,
        &harness,
        "2",
        Some("logo.png"),
        "image/png",
        b"\x89PNG",
    )
    .await;
    let app = strife_api::email_parts::router(pool, harness.storage.clone());
    let (status, headers, _) = get(
        app,
        &format!("/api/email/messages/{node_id}/parts/2?inline=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        header_value(&headers, "content-disposition")
            .expect("disposition")
            .starts_with("inline;")
    );
    let csp = header_value(&headers, "content-security-policy").expect("csp");
    assert!(csp.contains("default-src 'none'"), "{csp}");
    assert!(csp.contains("sandbox"), "{csp}");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_attachment_of_another_message_is_not_reachable(pool: PgPool) {
    let harness = harness().await;
    let owner = seed(
        &pool,
        &harness,
        "2",
        Some("a.pdf"),
        "application/pdf",
        b"secret",
    )
    .await;
    let other = seed(
        &pool,
        &harness,
        "2",
        Some("b.pdf"),
        "application/pdf",
        b"public",
    )
    .await;
    let app = strife_api::email_parts::router(pool, harness.storage.clone());

    // Each message serves only its own part, even though both use path "2".
    let (_, _, owner_body) = get(
        app.clone(),
        &format!("/api/email/messages/{owner}/parts/2"),
        None,
    )
    .await;
    let (_, _, other_body) = get(
        app.clone(),
        &format!("/api/email/messages/{other}/parts/2"),
        None,
    )
    .await;
    assert_eq!(owner_body, b"secret");
    assert_eq!(other_body, b"public");

    // A part path the message does not declare is not found, rather than
    // resolving to some other message's artifact.
    let (missing, _, _) = get(app, &format!("/api/email/messages/{owner}/parts/9.9"), None).await;
    assert_eq!(missing, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_deleted_message_stops_serving_its_attachments(pool: PgPool) {
    let harness = harness().await;
    let node_id = seed(
        &pool,
        &harness,
        "2",
        Some("a.pdf"),
        "application/pdf",
        b"bytes",
    )
    .await;
    let app = strife_api::email_parts::router(pool.clone(), harness.storage.clone());

    // Removing the projection is what deleting the message does to this table.
    sqlx::query("DELETE FROM email_messages WHERE node_id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("delete message");

    let (status, _, _) = get(app, &format!("/api/email/messages/{node_id}/parts/2"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn a_missing_artifact_reports_a_state_and_schedules_a_rebuild(pool: PgPool) {
    let harness = harness().await;
    let node_id = seed(
        &pool,
        &harness,
        "2",
        Some("a.pdf"),
        "application/pdf",
        b"bytes",
    )
    .await;
    // Mark the artifact failed, as materialization would on an unwritable part.
    upsert_email_attachment_artifact(
        &pool,
        &UpsertEmailAttachmentArtifact {
            node_id,
            part_path: "2",
            state: EmailArtifactState::Failed,
            storage_key: None,
            media_type: "application/pdf",
            byte_size: 0,
            checksum_sha256: None,
            depth: 0,
            is_message: false,
            materializer_version: "1",
            warnings: &[],
        },
    )
    .await
    .expect("mark failed");

    let app = strife_api::email_parts::router(pool.clone(), harness.storage.clone());
    let (status, _, body) = get(app, &format!("/api/email/messages/{node_id}/parts/2"), None).await;
    // Explicitly not a 404: the attachment exists, its bytes do not.
    assert_eq!(status, StatusCode::ACCEPTED);
    let parsed: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(parsed["status"], "failed");

    // Rebuilding is possible because the .eml original is canonical.
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE target_node_id = $1 AND job_type = 'email_extraction'",
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("count jobs");
    assert_eq!(queued, 1);
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_unsatisfiable_range_is_rejected(pool: PgPool) {
    let harness = harness().await;
    let node_id = seed(
        &pool,
        &harness,
        "2",
        Some("a.pdf"),
        "application/pdf",
        b"12345",
    )
    .await;
    let app = strife_api::email_parts::router(pool, harness.storage.clone());
    let (status, headers, _) = get(
        app,
        &format!("/api/email/messages/{node_id}/parts/2"),
        Some("bytes=99-200"),
    )
    .await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(header_value(&headers, "content-range"), Some("bytes */5"));
}

#[sqlx::test(migrations = "../db/migrations")]
async fn an_attachment_without_a_filename_still_downloads(pool: PgPool) {
    let harness = harness().await;
    let node_id = seed(&pool, &harness, "2.1", None, "application/pdf", b"bytes").await;
    let app = strife_api::email_parts::router(pool, harness.storage.clone());
    let (status, headers, _) = get(
        app,
        &format!("/api/email/messages/{node_id}/parts/2.1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // A nameless attachment gets a name derived from its part path, never an
    // empty or absent one.
    assert_eq!(
        header_value(&headers, "content-disposition"),
        Some("attachment; filename=\"attachment-2.1\"")
    );
}
