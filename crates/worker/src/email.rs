use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use strife_db::{
    EmailAddressInput, EmailAddressRole, EmailAttachmentInput, EmailExtractionStatus,
    EmailHeaderInput, EmailProjection, JobRecord, LifecycleState, UpsertEmailMessage,
    fail_job_terminal, get_file_object_by_node_id, get_node_by_id, replace_email_projection,
    skip_job,
};
use strife_media::{
    EMAIL_PARSER_NAME, EMAIL_PARSER_VERSION, EmailAddressKind, EmailParseLimits, ParsedEmail,
    detect_mime, is_rfc822_mime, looks_like_rfc822, parse_email,
};
use strife_storage::{StorageBackend, StorageKey};
use tokio::{io::AsyncWriteExt, time::timeout};
use tracing::{info, warn};
use uuid::Uuid;

use crate::JobHandler;

/// Global email parsing configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmailSettings {
    pub limits: EmailParseLimits,
    pub file_timeout: Duration,
}

impl Default for EmailSettings {
    fn default() -> Self {
        Self {
            limits: EmailParseLimits::default(),
            file_timeout: Duration::from_secs(120),
        }
    }
}

/// Turns managed `.eml` originals into durable structured projections.
pub struct EmailHandler {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    settings: EmailSettings,
}

impl EmailHandler {
    #[must_use]
    pub fn new(pool: PgPool, storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            pool,
            storage,
            settings: EmailSettings::default(),
        }
    }

    #[must_use]
    pub fn with_settings(mut self, settings: EmailSettings) -> Self {
        self.settings = settings;
        self
    }

    pub(crate) fn set_settings(&mut self, settings: EmailSettings) {
        self.settings = settings;
    }

    async fn process(&self, job: &JobRecord) -> Result<()> {
        let node = get_node_by_id(&self.pool, job.target_node_id)
            .await?
            .context("email target node no longer exists")?;
        if node.lifecycle_state != LifecycleState::Active {
            // Trashed files are skipped rather than failed: the file may be
            // restored, and a failure would consume the retry budget.
            let warning = "email parsing skipped because the file is trashed".to_owned();
            self.persist_empty(
                job.target_node_id,
                EmailExtractionStatus::Skipped,
                std::slice::from_ref(&warning),
            )
            .await?;
            skip_job(&self.pool, job.id, &warning).await?;
            return Ok(());
        }
        let file = get_file_object_by_node_id(&self.pool, job.target_node_id)
            .await?
            .context("email target has no finalized file object")?;
        let storage_id =
            Uuid::parse_str(&file.storage_key).context("invalid original storage key")?;

        let source = std::env::temp_dir().join(format!("strife-email-{}", Uuid::new_v4()));
        let started = Instant::now();
        let work = self.parse_and_store(job, storage_id, &source, started);
        let outcome = match timeout(self.settings.file_timeout, work).await {
            Ok(outcome) => outcome,
            Err(_) => Err(anyhow::anyhow!(
                "email parse timeout limit exceeded after {} seconds",
                self.settings.file_timeout.as_secs()
            )),
        };
        // Removed on every path — success, error, timeout, and cancellation.
        let _ = tokio::fs::remove_file(&source).await;

        match outcome {
            Ok(()) => Ok(()),
            Err(error) => {
                let detail = format!("{error:#}");
                if is_terminal(&detail) {
                    // Retrying cannot change a size or shape verdict, so the
                    // job is failed terminally instead of burning attempts.
                    warn!(
                        node_id = %job.target_node_id,
                        job_id = %job.id,
                        error = %detail,
                        "email parsing failed terminally"
                    );
                    self.persist_empty(
                        job.target_node_id,
                        terminal_status(&detail),
                        std::slice::from_ref(&detail),
                    )
                    .await?;
                    fail_job_terminal(&self.pool, job.id, &sanitize(&detail)).await?;
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn parse_and_store(
        &self,
        job: &JobRecord,
        storage_id: Uuid,
        source: &Path,
        started: Instant,
    ) -> Result<()> {
        copy_to_path(self.storage.as_ref(), storage_id, source).await?;
        let bytes = tokio::fs::read(source)
            .await
            .context("read staged email source")?;
        let source_bytes = bytes.len();

        // MIME is confirmed from content, not from the file extension or any
        // upload-declared type.
        let mime = detect_mime(source)?;
        if !is_rfc822_mime(&mime) && !looks_like_rfc822(&bytes) {
            let warning = format!("email parsing does not support MIME type {mime}");
            self.persist_empty(
                job.target_node_id,
                EmailExtractionStatus::Unsupported,
                std::slice::from_ref(&warning),
            )
            .await?;
            info!(
                node_id = %job.target_node_id,
                mime = %mime,
                "email parsing skipped a non-email file"
            );
            return Ok(());
        }

        let parsed = parse_email(&bytes, self.settings.limits)?;
        let duration_ms = i64::try_from(started.elapsed().as_millis())
            .context("email parse duration exceeds i64")?;
        self.persist(job.target_node_id, &parsed, duration_ms)
            .await?;

        info!(
            node_id = %job.target_node_id,
            job_id = %job.id,
            source_bytes,
            body_bytes = parsed.body_text.len(),
            attachment_count = parsed.attachments.len(),
            warning_count = parsed.warnings.len(),
            duration_ms,
            parser_version = EMAIL_PARSER_VERSION,
            "email parsed"
        );
        Ok(())
    }

    /// Writes the message and every dependent table in one transaction.
    async fn persist(&self, node_id: Uuid, parsed: &ParsedEmail, duration_ms: i64) -> Result<()> {
        let addresses: Vec<EmailAddressInput<'_>> = parsed
            .addresses
            .iter()
            .map(|address| EmailAddressInput {
                role: role_of(address.kind),
                display_name: address.display_name.as_deref(),
                address: &address.address,
            })
            .collect();
        let headers: Vec<EmailHeaderInput<'_>> = parsed
            .headers
            .iter()
            .map(|header| EmailHeaderInput {
                name: &header.name,
                value: &header.value,
            })
            .collect();
        let attachments: Vec<EmailAttachmentInput<'_>> = parsed
            .attachments
            .iter()
            .map(|attachment| EmailAttachmentInput {
                part_path: &attachment.part_path,
                filename: attachment.filename.as_deref(),
                media_type: &attachment.media_type,
                disposition: attachment.disposition.as_deref(),
                content_id: attachment.content_id.as_deref(),
                transfer_encoding: attachment.transfer_encoding.as_deref(),
                decoded_size: attachment.decoded_size,
                checksum_sha256: attachment.checksum_sha256.as_deref(),
                is_inline: attachment.is_inline,
                is_message: attachment.is_message,
                warnings: &attachment.warnings,
            })
            .collect();

        replace_email_projection(
            &self.pool,
            &EmailProjection {
                message: UpsertEmailMessage {
                    node_id,
                    status: EmailExtractionStatus::Completed,
                    parser_name: EMAIL_PARSER_NAME,
                    parser_version: EMAIL_PARSER_VERSION,
                    message_id: parsed.message_id.as_deref(),
                    normalized_message_id: parsed.normalized_message_id.as_deref(),
                    in_reply_to: parsed.in_reply_to.as_deref(),
                    reference_ids: &parsed.references,
                    subject: parsed.subject.as_deref(),
                    normalized_subject: parsed.normalized_subject.as_deref(),
                    sent_at: parsed.sent_at,
                    received_at: parsed.received_at,
                    body_text: &parsed.body_text,
                    body_html: parsed.body_html.as_deref(),
                    preview_text: &parsed.preview_text,
                    content_hash: Some(&parsed.content_hash),
                    provider_thread_id: parsed.provider_thread_id.as_deref(),
                    warnings: &parsed.warnings,
                    duration_ms: Some(duration_ms),
                },
                addresses: &addresses,
                headers: &headers,
                labels: &parsed.labels,
                attachments: &attachments,
            },
        )
        .await?;
        Ok(())
    }

    /// Records a terminal outcome that produced no message content.
    async fn persist_empty(
        &self,
        node_id: Uuid,
        status: EmailExtractionStatus,
        warnings: &[String],
    ) -> Result<()> {
        let empty_labels: Vec<String> = Vec::new();
        replace_email_projection(
            &self.pool,
            &EmailProjection {
                message: UpsertEmailMessage {
                    node_id,
                    status,
                    parser_name: EMAIL_PARSER_NAME,
                    parser_version: EMAIL_PARSER_VERSION,
                    message_id: None,
                    normalized_message_id: None,
                    in_reply_to: None,
                    reference_ids: &[],
                    subject: None,
                    normalized_subject: None,
                    sent_at: None,
                    received_at: None,
                    body_text: "",
                    body_html: None,
                    preview_text: "",
                    content_hash: None,
                    provider_thread_id: None,
                    warnings,
                    duration_ms: None,
                },
                addresses: &[],
                headers: &[],
                labels: &empty_labels,
                attachments: &[],
            },
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl JobHandler for EmailHandler {
    async fn handle(&self, job: &JobRecord) -> Result<()> {
        self.process(job).await
    }
}

const fn role_of(kind: EmailAddressKind) -> EmailAddressRole {
    match kind {
        EmailAddressKind::From => EmailAddressRole::From,
        EmailAddressKind::Sender => EmailAddressRole::Sender,
        EmailAddressKind::ReplyTo => EmailAddressRole::ReplyTo,
        EmailAddressKind::To => EmailAddressRole::To,
        EmailAddressKind::Cc => EmailAddressRole::Cc,
        EmailAddressKind::Bcc => EmailAddressRole::Bcc,
    }
}

/// Failures a retry cannot resolve.
fn is_terminal(detail: &str) -> bool {
    detail.contains("limit exceeded")
        || detail.contains("not an RFC 5322 message")
        || detail.contains("could not be parsed")
}

fn terminal_status(detail: &str) -> EmailExtractionStatus {
    if detail.contains("not an RFC 5322 message") {
        EmailExtractionStatus::Unsupported
    } else {
        EmailExtractionStatus::Failed
    }
}

/// Strips anything that could carry message content out of an API-visible
/// error. Underlying causes stay in the logs and the stored warning.
fn sanitize(detail: &str) -> String {
    detail
        .lines()
        .next()
        .unwrap_or("email parsing failed")
        .chars()
        .take(200)
        .collect()
}

async fn copy_to_path(storage: &dyn StorageBackend, storage_id: Uuid, path: &Path) -> Result<()> {
    let mut reader = storage.get_stream(StorageKey::original(storage_id)).await?;
    let mut file = tokio::fs::File::create(path)
        .await
        .context("create email source file")?;
    tokio::io::copy(&mut reader, &mut file)
        .await
        .context("copy original for email parsing")?;
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_terminal, sanitize, terminal_status};
    use strife_db::EmailExtractionStatus;

    #[test]
    fn size_and_shape_failures_are_terminal() {
        assert!(is_terminal("email source size limit exceeded: 99 bytes"));
        assert!(is_terminal("input is not an RFC 5322 message"));
        assert!(is_terminal("message could not be parsed"));
        // A transient storage or database problem must stay retryable.
        assert!(!is_terminal("connection reset by peer"));
        assert!(!is_terminal("copy original for email parsing"));
    }

    #[test]
    fn non_email_input_is_unsupported_not_failed() {
        assert_eq!(
            terminal_status("input is not an RFC 5322 message"),
            EmailExtractionStatus::Unsupported
        );
        assert_eq!(
            terminal_status("email source size limit exceeded"),
            EmailExtractionStatus::Failed
        );
    }

    #[test]
    fn sanitized_errors_stay_single_line_and_bounded() {
        let detail = format!("first line\nsecond line with {}", "x".repeat(500));
        let sanitized = sanitize(&detail);
        assert_eq!(sanitized, "first line");
        assert!(!sanitized.contains('\n'));
        assert!(sanitize(&"y".repeat(500)).len() <= 200);
    }
}
