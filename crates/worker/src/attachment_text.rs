//! Extracts searchable text from materialized attachment artifacts.
//!
//! Reuses the adapters the rest of Strife already runs — Tika for embedded
//! document text, the OCR pipeline for scans and images — rather than
//! introducing a second set of extractors that would drift from them. The
//! routing decision mirrors the OCR handler's: a PDF is asked for its embedded
//! text first, and only rasterized when that text is too thin to be real.
//!
//! One job covers one message's attachments. The queue targets nodes and an
//! attachment is a MIME part rather than a node, so per-attachment jobs would
//! need a parallel queue for no benefit; a message's attachments are also the
//! natural unit for reindexing, since they contribute to one search vector.
//!
//! A failed or unsupported attachment never changes the message's own
//! extraction state. The message parsed, its body is searchable, and a PDF that
//! Tika could not read does not undo any of that.

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use strife_db::{
    EmailArtifactState, EmailAttachmentArtifact, EmailAttachmentTextInput,
    EmailAttachmentTextOutcome, EmailAttachmentTextSource, EmailExtractionStatus, JobRecord,
    list_email_attachment_artifacts, replace_email_attachment_text,
};
use strife_media::{
    OcrNormalizationLimits, extract_embedded_pdf_text, extract_ocr, extract_tika,
    is_supported_ocr_mime, normalize_ocr_input,
};
use strife_storage::{StorageBackend, StorageKey};
use tokio::{io::AsyncWriteExt, time::timeout};
use tracing::{info, warn};
use uuid::Uuid;

use crate::JobHandler;

/// Identifies which extractor produced a row, so a version change can drive
/// bounded reprocessing rather than a full rebuild.
pub const ATTACHMENT_EXTRACTOR_VERSION: &str = "1";
const TIKA_EXTRACTOR: &str = "tika";
const OCR_EXTRACTOR: &str = "tesseract";

/// Bounds on attachment text extraction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentTextSettings {
    /// Largest text stored for one attachment. Text beyond this is dropped with
    /// a warning: an indexed prefix still finds the document, and an unbounded
    /// column would let one pathological attachment dominate the index.
    pub max_text_bytes: usize,
    /// Below this many non-whitespace characters a PDF's embedded text is
    /// treated as absent and the page is rasterized instead.
    pub minimum_embedded_text_chars: usize,
    pub max_pages: u32,
    pub raster_dpi: u32,
    pub max_pixels_per_page: u64,
    pub memory_limit_bytes: u64,
    /// Applies to one attachment, not to the whole message.
    pub attachment_timeout: Duration,
}

impl Default for AttachmentTextSettings {
    /// Provisional, to be profiled on Orion before being treated as final.
    fn default() -> Self {
        Self {
            max_text_bytes: 1024 * 1024,
            minimum_embedded_text_chars: 20,
            max_pages: 50,
            raster_dpi: 200,
            max_pixels_per_page: 40_000_000,
            memory_limit_bytes: 512 * 1024 * 1024,
            attachment_timeout: Duration::from_secs(300),
        }
    }
}

/// Office formats Tika can read text out of directly.
fn is_tika_document(mime: &str) -> bool {
    matches!(
        mime,
        "application/msword"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "application/vnd.ms-excel"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            | "application/vnd.ms-powerpoint"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            | "application/vnd.oasis.opendocument.text"
            | "application/vnd.oasis.opendocument.spreadsheet"
            | "application/rtf"
    )
}

/// Text formats worth indexing without invoking an extractor at all.
fn is_plain_text(mime: &str) -> bool {
    mime.starts_with("text/") || mime == "application/json" || mime == "application/xml"
}

/// Extracts text from a message's stored attachments.
pub struct AttachmentTextHandler {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    tika_url: String,
    settings: AttachmentTextSettings,
}

impl AttachmentTextHandler {
    #[must_use]
    pub fn new(pool: PgPool, storage: Arc<dyn StorageBackend>, tika_url: String) -> Self {
        Self {
            pool,
            storage,
            tika_url,
            settings: AttachmentTextSettings::default(),
        }
    }

    #[must_use]
    pub fn with_settings(mut self, settings: AttachmentTextSettings) -> Self {
        self.settings = settings;
        self
    }

    async fn process(&self, job: &JobRecord) -> Result<()> {
        let artifacts = list_email_attachment_artifacts(&self.pool, job.target_node_id)
            .await
            .context("list attachment artifacts")?;
        let mut extracted = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;

        for artifact in artifacts {
            if artifact.state != EmailArtifactState::Ready {
                continue;
            }
            // Already-done work is left alone so a rerun of the message is cheap;
            // reprocessing resets the status to make attachments eligible again.
            if artifact.text_status != EmailExtractionStatus::Pending {
                continue;
            }
            match self.extract_one(job.target_node_id, &artifact).await {
                Ok(true) => extracted += 1,
                Ok(false) => skipped += 1,
                Err(error) => {
                    let detail = format!("{error:#}");
                    warn!(
                        node_id = %job.target_node_id,
                        part_path = %artifact.part_path,
                        error = %detail,
                        "attachment text extraction failed"
                    );
                    // Recorded as a failure on the attachment only. The message
                    // keeps its own completed state and stays searchable.
                    replace_email_attachment_text(
                        &self.pool,
                        job.target_node_id,
                        &artifact.part_path,
                        &[],
                        &EmailAttachmentTextOutcome {
                            status: EmailExtractionStatus::Failed,
                            extractor_name: None,
                            extractor_version: Some(ATTACHMENT_EXTRACTOR_VERSION),
                            warnings: std::slice::from_ref(&detail),
                        },
                    )
                    .await?;
                    failed += 1;
                }
            }
        }

        info!(
            node_id = %job.target_node_id,
            job_id = %job.id,
            extracted,
            skipped,
            failed,
            "attachment text extraction completed"
        );
        Ok(())
    }

    /// Returns whether the attachment produced any text.
    async fn extract_one(&self, node_id: Uuid, artifact: &EmailAttachmentArtifact) -> Result<bool> {
        let Some(storage_key) = artifact.storage_key.as_deref() else {
            return Ok(false);
        };
        let media_type = artifact.media_type.as_str();
        let supported = is_plain_text(media_type)
            || is_tika_document(media_type)
            || is_supported_ocr_mime(media_type);
        if !supported {
            // An unsupported attachment is a fact about the format, not a
            // failure, and retrying cannot change it.
            replace_email_attachment_text(
                &self.pool,
                node_id,
                &artifact.part_path,
                &[],
                &EmailAttachmentTextOutcome {
                    status: EmailExtractionStatus::Unsupported,
                    extractor_name: None,
                    extractor_version: Some(ATTACHMENT_EXTRACTOR_VERSION),
                    warnings: &[],
                },
            )
            .await?;
            return Ok(false);
        }

        let source = std::env::temp_dir().join(format!("strife-attach-{}", Uuid::new_v4()));
        let work = self.extract_to_pages(storage_key, media_type, &source);
        let outcome = match timeout(self.settings.attachment_timeout, work).await {
            Ok(result) => result,
            Err(_) => Err(anyhow::anyhow!(
                "attachment extraction timeout exceeded after {} seconds",
                self.settings.attachment_timeout.as_secs()
            )),
        };
        let _ = tokio::fs::remove_file(&source).await;
        let (pages, extractor, mut warnings) = outcome?;

        let (pages, truncated) = self.bound(pages);
        if truncated {
            warnings.push(format!(
                "attachment text truncated at {} bytes",
                self.settings.max_text_bytes
            ));
        }
        let produced = !pages.is_empty();
        let inputs: Vec<EmailAttachmentTextInput<'_>> = pages
            .iter()
            .map(|page| EmailAttachmentTextInput {
                page_number: page.page_number,
                content: &page.content,
                source: page.source,
                confidence: page.confidence,
            })
            .collect();

        replace_email_attachment_text(
            &self.pool,
            node_id,
            &artifact.part_path,
            &inputs,
            &EmailAttachmentTextOutcome {
                status: EmailExtractionStatus::Completed,
                extractor_name: Some(extractor),
                extractor_version: Some(ATTACHMENT_EXTRACTOR_VERSION),
                warnings: &warnings,
            },
        )
        .await?;
        Ok(produced)
    }

    /// Applies the stored-text ceiling across an attachment's pages in order.
    fn bound(&self, pages: Vec<ExtractedPage>) -> (Vec<ExtractedPage>, bool) {
        let mut kept = Vec::new();
        let mut used = 0usize;
        let mut truncated = false;
        for mut page in pages {
            if used >= self.settings.max_text_bytes {
                truncated = true;
                break;
            }
            let remaining = self.settings.max_text_bytes - used;
            if page.content.len() > remaining {
                // Cut on a character boundary so the stored text stays valid
                // UTF-8; a byte-exact cut could split a multi-byte character.
                let mut end = remaining;
                while end > 0 && !page.content.is_char_boundary(end) {
                    end -= 1;
                }
                page.content.truncate(end);
                truncated = true;
            }
            used += page.content.len();
            if !page.content.trim().is_empty() {
                kept.push(page);
            }
        }
        (kept, truncated)
    }

    async fn extract_to_pages(
        &self,
        storage_key: &str,
        media_type: &str,
        source: &Path,
    ) -> Result<(Vec<ExtractedPage>, &'static str, Vec<String>)> {
        self.copy_artifact(storage_key, source).await?;

        if is_plain_text(media_type) {
            let content = tokio::fs::read_to_string(source)
                .await
                .unwrap_or_else(|_| String::new());
            return Ok((
                vec![ExtractedPage {
                    page_number: 1,
                    content,
                    source: EmailAttachmentTextSource::Embedded,
                    confidence: None,
                }],
                TIKA_EXTRACTOR,
                Vec::new(),
            ));
        }

        if media_type == "application/pdf" {
            let embedded = extract_embedded_pdf_text(
                source,
                &self.tika_url,
                self.settings.minimum_embedded_text_chars,
            )
            .await?;
            if embedded.usable {
                return Ok((
                    vec![ExtractedPage {
                        page_number: 1,
                        content: embedded.content,
                        source: EmailAttachmentTextSource::Embedded,
                        confidence: None,
                    }],
                    TIKA_EXTRACTOR,
                    embedded.warnings,
                ));
            }
            // Falls through to OCR: a PDF whose embedded text is too thin is a
            // scan, and the same routing the OCR handler uses applies here.
        } else if is_tika_document(media_type) {
            let result = extract_tika(source, &self.tika_url).await?;
            let content = result
                .raw_payload
                .get("X-TIKA:content")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_owned();
            return Ok((
                vec![ExtractedPage {
                    page_number: 1,
                    content,
                    source: EmailAttachmentTextSource::Embedded,
                    confidence: None,
                }],
                TIKA_EXTRACTOR,
                result.warnings,
            ));
        }

        if !is_supported_ocr_mime(media_type) {
            return Ok((Vec::new(), TIKA_EXTRACTOR, Vec::new()));
        }
        self.ocr_pages(source, media_type).await
    }

    async fn ocr_pages(
        &self,
        source: &Path,
        media_type: &str,
    ) -> Result<(Vec<ExtractedPage>, &'static str, Vec<String>)> {
        let normalized = normalize_ocr_input(
            source,
            media_type,
            OcrNormalizationLimits {
                raster_dpi: self.settings.raster_dpi,
                max_pages: self.settings.max_pages,
                max_pixels_per_page: self.settings.max_pixels_per_page,
                memory_limit_bytes: self.settings.memory_limit_bytes,
                process_timeout: self.settings.attachment_timeout,
            },
        )
        .await?;

        let mut pages = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        for normalized_page in &normalized.pages {
            let output = extract_ocr(
                &normalized_page.path,
                "tesseract",
                "eng",
                "unverified",
                self.settings.attachment_timeout,
                self.settings.max_text_bytes.saturating_mul(8),
            )
            .await?;
            warnings.extend(output.warnings.iter().cloned());
            for page in output.pages {
                pages.push(ExtractedPage {
                    page_number: normalized_page.page_number,
                    content: page.content,
                    source: EmailAttachmentTextSource::Ocr,
                    confidence: page.confidence,
                });
            }
        }
        Ok((pages, OCR_EXTRACTOR, warnings))
    }

    async fn copy_artifact(&self, storage_key: &str, destination: &Path) -> Result<()> {
        use tokio::io::AsyncReadExt;
        let id = Uuid::parse_str(storage_key).context("invalid artifact storage key")?;
        let mut reader = self
            .storage
            .get_stream(StorageKey::artifact(id))
            .await
            .context("open attachment artifact")?;
        let mut file = tokio::fs::File::create(destination)
            .await
            .context("create attachment scratch file")?;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer).await.context("read artifact")?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .await
                .context("write attachment scratch file")?;
        }
        file.flush().await.context("flush scratch file")?;
        Ok(())
    }
}

struct ExtractedPage {
    page_number: i32,
    content: String,
    source: EmailAttachmentTextSource,
    confidence: Option<f32>,
}

#[async_trait]
impl JobHandler for AttachmentTextHandler {
    async fn handle(&self, job: &JobRecord) -> Result<()> {
        self.process(job).await
    }
}
