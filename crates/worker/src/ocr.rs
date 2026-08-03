use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use sqlx::PgPool;
use strife_db::{
    DocumentTextPageInput, DocumentTextSource, DocumentTextStatus, JobRecord, JobType,
    LifecycleState, UpsertDocumentText, append_ocr_event, fail_job_terminal,
    get_file_object_by_node_id, get_node_by_id, replace_document_text, skip_job,
};
use strife_media::{
    OcrNormalizationLimits, OcrPage, detect_mime, extract_embedded_pdf_text, extract_ocr,
    normalize_ocr_input,
};
use strife_storage::{StorageBackend, StorageKey};
use tokio::{io::AsyncWriteExt, time::timeout};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{JobHandler, metadata::is_ocr_candidate_mime};

const EMBEDDED_TEXT_ENGINE: &str = "tika";
const EMBEDDED_TEXT_ENGINE_VERSION: &str = "server";

/// Global OCR engine and safety configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OcrSettings {
    pub language: String,
    pub tesseract_binary: String,
    pub engine_version: String,
    pub minimum_embedded_text_chars: usize,
    pub raster_dpi: u32,
    pub max_pages: u32,
    pub max_pixels_per_page: u64,
    pub file_timeout: Duration,
    pub memory_limit_bytes: u64,
    pub max_text_bytes: usize,
}

impl Default for OcrSettings {
    fn default() -> Self {
        Self {
            language: "eng".to_owned(),
            tesseract_binary: "tesseract".to_owned(),
            engine_version: "unverified".to_owned(),
            minimum_embedded_text_chars: 20,
            raster_dpi: 200,
            max_pages: 100,
            max_pixels_per_page: 40_000_000,
            file_timeout: Duration::from_secs(600),
            memory_limit_bytes: 512 * 1024 * 1024,
            max_text_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Durable OCR worker, including embedded-PDF detection and raster OCR routing.
pub struct OcrHandler {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    tika_url: String,
    settings: OcrSettings,
}

impl OcrHandler {
    #[must_use]
    pub fn new(
        pool: PgPool,
        storage: Arc<dyn StorageBackend>,
        tika_url: String,
        minimum_embedded_text_chars: usize,
    ) -> Self {
        let settings = OcrSettings {
            minimum_embedded_text_chars,
            ..OcrSettings::default()
        };
        Self {
            pool,
            storage,
            tika_url,
            settings,
        }
    }

    pub(crate) fn set_minimum_embedded_text_chars(&mut self, minimum_chars: usize) {
        self.settings.minimum_embedded_text_chars = minimum_chars;
    }

    pub(crate) fn set_settings(&mut self, settings: OcrSettings) {
        self.settings = settings;
    }

    /// Applies explicit OCR settings, primarily for controlled workers and integration tests.
    #[must_use]
    pub fn with_settings(mut self, settings: OcrSettings) -> Self {
        self.settings = settings;
        self
    }

    async fn process(&self, job: &JobRecord) -> Result<()> {
        let node = get_node_by_id(&self.pool, job.target_node_id)
            .await?
            .context("OCR target node no longer exists")?;
        if node.lifecycle_state != LifecycleState::Active {
            let warning = "OCR skipped because the file is trashed".to_owned();
            self.persist_empty(
                job.target_node_id,
                DocumentTextStatus::Skipped,
                std::slice::from_ref(&warning),
            )
            .await?;
            skip_job(&self.pool, job.id, &warning).await?;
            append_ocr_event(
                &self.pool,
                job.target_node_id,
                "skipped",
                None,
                None,
                Some(&warning),
            )
            .await?;
            return Ok(());
        }
        append_ocr_event(&self.pool, job.target_node_id, "running", None, None, None).await?;
        let file = get_file_object_by_node_id(&self.pool, job.target_node_id)
            .await?
            .context("OCR target has no finalized file object")?;
        let storage_id =
            Uuid::parse_str(&file.storage_key).context("invalid original storage key")?;
        let source = std::env::temp_dir().join(format!("strife-ocr-source-{}", Uuid::new_v4()));
        let started = Instant::now();
        let result = async {
            copy_to_path(self.storage.as_ref(), storage_id, &source).await?;
            let mime = detect_mime(&source)?;
            if !is_ocr_candidate_mime(&mime) {
                let warning = format!("OCR does not support MIME type {mime}");
                self.persist_empty(
                    job.target_node_id,
                    DocumentTextStatus::Unsupported,
                    std::slice::from_ref(&warning),
                )
                .await?;
                append_ocr_event(
                    &self.pool,
                    job.target_node_id,
                    "unsupported",
                    None,
                    None,
                    Some(&warning),
                )
                .await?;
                return Ok(());
            }
            if mime == "application/pdf" {
                let embedded = extract_embedded_pdf_text(
                    &source,
                    &self.tika_url,
                    self.settings.minimum_embedded_text_chars,
                )
                .await?;
                if embedded.usable {
                    self.persist_embedded(job, &embedded.content, &embedded.warnings, started)
                        .await?;
                    return Ok(());
                }
            }
            self.run_raster_ocr(job.target_node_id, &source, &mime, started)
                .await
        };
        let outcome = match timeout(self.settings.file_timeout, result).await {
            Ok(outcome) => outcome,
            Err(_) => Err(anyhow::anyhow!(
                "OCR file timeout limit exceeded after {} seconds",
                self.settings.file_timeout.as_secs()
            )),
        };
        let _ = tokio::fs::remove_file(&source).await;
        outcome
    }

    async fn persist_embedded(
        &self,
        job: &JobRecord,
        content: &str,
        warnings: &[String],
        started: Instant,
    ) -> Result<()> {
        let char_count = i32::try_from(content.chars().count())
            .context("embedded PDF text exceeds the database character-count range")?;
        replace_document_text(
            &self.pool,
            &UpsertDocumentText {
                node_id: job.target_node_id,
                source: DocumentTextSource::Embedded,
                status: DocumentTextStatus::Completed,
                language: "und",
                engine_name: EMBEDDED_TEXT_ENGINE,
                engine_version: EMBEDDED_TEXT_ENGINE_VERSION,
                page_count: Some(1),
                mean_confidence: None,
                char_count,
                warnings,
                duration_ms: Some(duration_millis(started)?),
            },
            &[DocumentTextPageInput {
                page_number: 1,
                content,
                confidence: None,
                width: None,
                height: None,
            }],
        )
        .await?;
        skip_job(
            &self.pool,
            job.id,
            "OCR skipped because the PDF contains usable embedded text",
        )
        .await?
        .context("OCR job was no longer leased when recording the skip")?;
        append_ocr_event(
            &self.pool,
            job.target_node_id,
            "skipped",
            Some(1),
            None,
            None,
        )
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn run_raster_ocr(
        &self,
        node_id: Uuid,
        source: &Path,
        mime: &str,
        started: Instant,
    ) -> Result<()> {
        let normalized = normalize_ocr_input(
            source,
            mime,
            OcrNormalizationLimits {
                raster_dpi: self.settings.raster_dpi,
                max_pages: self.settings.max_pages,
                max_pixels_per_page: self.settings.max_pixels_per_page,
                memory_limit_bytes: self.settings.memory_limit_bytes,
                process_timeout: self.settings.file_timeout,
            },
        )
        .await?;
        let mut pages = Vec::with_capacity(normalized.pages.len());
        let mut warnings = Vec::new();
        let mut peak_memory_bytes: Option<u64> = None;
        for normalized_page in &normalized.pages {
            let output = extract_ocr(
                &normalized_page.path,
                &self.settings.tesseract_binary,
                &self.settings.language,
                &self.settings.engine_version,
                self.settings.file_timeout,
                self.settings.max_text_bytes.saturating_mul(8),
            )
            .await?;
            if let Some(bytes) = output.peak_memory_bytes {
                peak_memory_bytes = Some(peak_memory_bytes.unwrap_or_default().max(bytes));
            }
            warnings.extend(output.warnings);
            let page = output.pages.into_iter().next().unwrap_or(OcrPage {
                page_number: normalized_page.page_number,
                content: String::new(),
                confidence: None,
                width: Some(i32::try_from(normalized_page.width)?),
                height: Some(i32::try_from(normalized_page.height)?),
            });
            pages.push(OcrPage {
                page_number: normalized_page.page_number,
                width: Some(i32::try_from(normalized_page.width)?),
                height: Some(i32::try_from(normalized_page.height)?),
                ..page
            });
        }
        let text_bytes = pages
            .iter()
            .map(|page| page.content.len())
            .try_fold(0_usize, usize::checked_add)
            .context("OCR text byte count overflowed")?;
        if text_bytes > self.settings.max_text_bytes {
            bail!(
                "OCR stored text limit exceeded: {text_bytes} bytes is greater than {}",
                self.settings.max_text_bytes
            );
        }
        let char_count = pages
            .iter()
            .map(|page| page.content.chars().count())
            .sum::<usize>();
        let mean_confidence = weighted_mean_confidence(&pages);
        let page_inputs = pages
            .iter()
            .map(|page| DocumentTextPageInput {
                page_number: page.page_number,
                content: &page.content,
                confidence: page.confidence,
                width: page.width,
                height: page.height,
            })
            .collect::<Vec<_>>();
        replace_document_text(
            &self.pool,
            &UpsertDocumentText {
                node_id,
                source: DocumentTextSource::Ocr,
                status: DocumentTextStatus::Completed,
                language: &self.settings.language,
                engine_name: "tesseract",
                engine_version: &self.settings.engine_version,
                page_count: Some(i32::try_from(pages.len())?),
                mean_confidence,
                char_count: i32::try_from(char_count)?,
                warnings: &warnings,
                duration_ms: Some(duration_millis(started)?),
            },
            &page_inputs,
        )
        .await?;
        append_ocr_event(
            &self.pool,
            node_id,
            "completed",
            Some(i32::try_from(pages.len())?),
            mean_confidence,
            warnings.first().map(String::as_str),
        )
        .await?;
        info!(
            %node_id,
            duration_ms = duration_millis(started)?,
            text_bytes,
            ?peak_memory_bytes,
            "OCR completed"
        );
        Ok(())
    }

    async fn persist_empty(
        &self,
        node_id: Uuid,
        status: DocumentTextStatus,
        warnings: &[String],
    ) -> Result<()> {
        replace_document_text(
            &self.pool,
            &UpsertDocumentText {
                node_id,
                source: DocumentTextSource::Ocr,
                status,
                language: &self.settings.language,
                engine_name: "tesseract",
                engine_version: &self.settings.engine_version,
                page_count: None,
                mean_confidence: None,
                char_count: 0,
                warnings,
                duration_ms: None,
            },
            &[],
        )
        .await?;
        Ok(())
    }

    async fn record_failure(&self, job: &JobRecord, error: &anyhow::Error) -> Result<bool> {
        let warning = format!("{error:#}");
        if let Err(persist_error) = self
            .persist_empty(
                job.target_node_id,
                DocumentTextStatus::Failed,
                std::slice::from_ref(&warning),
            )
            .await
        {
            warn!(%persist_error, node_id = %job.target_node_id, "could not persist OCR failure");
        }
        if get_node_by_id(&self.pool, job.target_node_id)
            .await?
            .is_some()
        {
            append_ocr_event(
                &self.pool,
                job.target_node_id,
                "failed",
                None,
                None,
                Some(&warning),
            )
            .await?;
        }
        let terminal =
            warning.contains(" limit exceeded") || warning.contains("does not support MIME");
        if terminal {
            fail_job_terminal(&self.pool, job.id, &warning).await?;
        }
        Ok(terminal)
    }
}

#[async_trait]
impl JobHandler for OcrHandler {
    async fn handle(&self, job: &JobRecord) -> Result<()> {
        if job.job_type != JobType::Ocr {
            bail!("unsupported job type: {:?}", job.job_type);
        }
        match self.process(job).await {
            Ok(()) => Ok(()),
            Err(error) => {
                if self.record_failure(job, &error).await? {
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }
}

async fn copy_to_path(storage: &dyn StorageBackend, storage_id: Uuid, path: &Path) -> Result<()> {
    let mut reader = storage.get_stream(StorageKey::original(storage_id)).await?;
    let mut file = tokio::fs::File::create(path)
        .await
        .context("create OCR source file")?;
    tokio::io::copy(&mut reader, &mut file)
        .await
        .context("copy original for OCR")?;
    file.flush().await?;
    Ok(())
}

fn duration_millis(started: Instant) -> Result<i64> {
    i64::try_from(started.elapsed().as_millis()).context("OCR duration exceeds i64")
}

fn weighted_mean_confidence(pages: &[OcrPage]) -> Option<f32> {
    let mut weighted = 0.0_f64;
    let mut characters = 0_u32;
    for page in pages {
        let Some(confidence) = page.confidence else {
            continue;
        };
        let count = u32::try_from(page.content.chars().count()).ok()?;
        weighted += f64::from(confidence) * f64::from(count);
        characters = characters.checked_add(count)?;
    }
    (characters > 0).then(|| confidence_as_f32(weighted / f64::from(characters)))
}

#[allow(clippy::cast_possible_truncation)]
fn confidence_as_f32(value: f64) -> f32 {
    value as f32
}
