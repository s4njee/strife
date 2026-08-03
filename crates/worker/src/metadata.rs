use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use sqlx::PgPool;
use strife_db::{
    ArtifactState, ArtifactType, DerivedArtifactRecord, JobRecord, JobType, MediaStreamInput,
    MediaStreamType, UpsertArtifact, create_or_update_artifact, get_artifact,
    get_file_object_by_node_id, replace_media_streams,
};
use strife_media::{
    StreamType, convert_office_to_pdf, detect_mime, extract_exif, extract_ffprobe, extract_tika,
    generate_image_preview, generate_thumbnail,
};
use strife_storage::{StorageBackend, StorageKey};
use tokio::{io::AsyncWriteExt, sync::Semaphore};
use uuid::Uuid;

use crate::JobHandler;

/// Metadata job implementation with an independent concurrency gate per external extractor.
pub struct MetadataHandler {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
    tika_url: String,
    exiftool_slots: Semaphore,
    ffprobe_slots: Semaphore,
    tika_slots: Semaphore,
    preview_slots: Semaphore,
}

impl MetadataHandler {
    #[must_use]
    pub fn new(
        pool: PgPool,
        storage: Arc<dyn StorageBackend>,
        tika_url: String,
        extractor_concurrency: usize,
        preview_concurrency: usize,
    ) -> Self {
        Self {
            pool,
            storage,
            tika_url,
            exiftool_slots: Semaphore::new(extractor_concurrency),
            ffprobe_slots: Semaphore::new(extractor_concurrency),
            tika_slots: Semaphore::new(extractor_concurrency),
            preview_slots: Semaphore::new(preview_concurrency),
        }
    }

    async fn generate_previews(&self, job: &JobRecord) -> Result<()> {
        let _permit = self.preview_slots.acquire().await?;
        let file = get_file_object_by_node_id(&self.pool, job.target_node_id)
            .await?
            .context("preview target has no finalized file object")?;
        let storage_id =
            Uuid::parse_str(&file.storage_key).context("invalid original storage key")?;
        let source = std::env::temp_dir().join(format!("strife-preview-source-{}", Uuid::new_v4()));
        let result = async {
            copy_to_path(self.storage.as_ref(), storage_id, &source).await?;
            let mime = detect_mime(&source)?;
            let mut found = false;
            for artifact_type in [ArtifactType::Thumbnail, ArtifactType::Preview] {
                let Some(artifact) =
                    get_artifact(&self.pool, job.target_node_id, artifact_type).await?
                else {
                    continue;
                };
                if artifact.state == ArtifactState::Failed {
                    bail!("preview artifact previously failed");
                }
                if artifact.state != ArtifactState::Generating {
                    continue;
                }
                found = true;
                if let Err(error) = self.generate_artifact(&artifact, &source, &mime).await {
                    self.mark_artifact_failed(&artifact).await?;
                    return Err(error);
                }
            }
            if !found {
                bail!("preview job has no generating artifact");
            }
            Ok(())
        }
        .await;
        let _ = tokio::fs::remove_file(&source).await;
        result
    }

    async fn generate_artifact(
        &self,
        artifact: &DerivedArtifactRecord,
        source: &Path,
        mime: &str,
    ) -> Result<()> {
        let output = std::env::temp_dir().join(format!("strife-preview-output-{}", Uuid::new_v4()));
        let generated = async {
            let (format, width, height, byte_size) = match artifact.artifact_type {
                ArtifactType::Thumbnail => {
                    let generated = generate_thumbnail(source, &output, 256).await?;
                    (
                        generated.format,
                        Some(i32::try_from(generated.width)?),
                        Some(i32::try_from(generated.height)?),
                        i64::try_from(generated.byte_size)?,
                    )
                }
                ArtifactType::Preview if is_office_mime(mime) => {
                    let size = convert_office_to_pdf(source, &output).await?;
                    (
                        "application/pdf".to_owned(),
                        None,
                        None,
                        i64::try_from(size)?,
                    )
                }
                ArtifactType::Preview if mime.starts_with("image/") => {
                    let generated = generate_image_preview(source, &output).await?;
                    (
                        generated.format,
                        Some(i32::try_from(generated.width)?),
                        Some(i32::try_from(generated.height)?),
                        i64::try_from(generated.byte_size)?,
                    )
                }
                ArtifactType::Preview => bail!("preview generation is unsupported for {mime}"),
            };
            let key =
                Uuid::parse_str(&artifact.storage_key).context("invalid artifact storage key")?;
            let reader = tokio::fs::File::open(&output)
                .await
                .context("open generated artifact")?;
            self.storage
                .put_stream(StorageKey::artifact(key), Box::pin(reader))
                .await?;
            create_or_update_artifact(
                &self.pool,
                &UpsertArtifact {
                    node_id: artifact.node_id,
                    artifact_type: artifact.artifact_type,
                    format: &format,
                    width,
                    height,
                    storage_key: &artifact.storage_key,
                    byte_size,
                    generator_version: "preview-v1",
                    state: ArtifactState::Ready,
                },
            )
            .await?;
            Ok(())
        }
        .await;
        let _ = tokio::fs::remove_file(&output).await;
        generated
    }

    async fn mark_artifact_failed(&self, artifact: &DerivedArtifactRecord) -> Result<()> {
        create_or_update_artifact(
            &self.pool,
            &UpsertArtifact {
                node_id: artifact.node_id,
                artifact_type: artifact.artifact_type,
                format: &artifact.format,
                width: artifact.width,
                height: artifact.height,
                storage_key: &artifact.storage_key,
                byte_size: artifact.byte_size,
                generator_version: "preview-v1",
                state: ArtifactState::Failed,
            },
        )
        .await?;
        Ok(())
    }

    async fn extract(&self, job: &JobRecord) -> Result<()> {
        let file = get_file_object_by_node_id(&self.pool, job.target_node_id)
            .await?
            .context("metadata target has no finalized file object")?;
        let storage_id =
            Uuid::parse_str(&file.storage_key).context("invalid original storage key")?;
        let temporary_path =
            std::env::temp_dir().join(format!("strife-metadata-{}", Uuid::new_v4()));
        let result = async {
            copy_to_path(self.storage.as_ref(), storage_id, &temporary_path).await?;
            let mime = detect_mime(&temporary_path)?;
            let identity_payload = serde_json::json!({
                "mime": mime,
                "size": file.byte_size,
                "checksum": file.checksum_sha256,
            });
            persist_record(
                &self.pool,
                job.target_node_id,
                "mime",
                "libmagic-v1",
                "completed",
                &identity_payload,
                &[],
            )
            .await?;
            upsert_base_metadata(&self.pool, job.target_node_id, &mime).await?;

            if mime.starts_with("image/") {
                let _permit = self.exiftool_slots.acquire().await?;
                self.extract_image(job, &temporary_path).await
            } else if mime.starts_with("video/") || mime.starts_with("audio/") {
                let _permit = self.ffprobe_slots.acquire().await?;
                self.extract_media(job, &temporary_path).await
            } else if is_document_mime(&mime) {
                let _permit = self.tika_slots.acquire().await?;
                self.extract_document(job, &temporary_path).await
            } else {
                persist_record(
                    &self.pool,
                    job.target_node_id,
                    "generic",
                    "adapter-v1",
                    "unsupported",
                    &identity_payload,
                    &["no specialized extractor supports this MIME type".to_owned()],
                )
                .await
            }
        }
        .await;
        let _ = tokio::fs::remove_file(&temporary_path).await;
        result
    }

    async fn extract_image(&self, job: &JobRecord, path: &Path) -> Result<()> {
        let exif = match extract_exif(path).await {
            Ok(result) => result,
            Err(error) => return extractor_failed(&self.pool, job, "exiftool", error).await,
        };
        persist_record(
            &self.pool,
            job.target_node_id,
            "exiftool",
            "adapter-v1",
            "completed",
            &exif.raw_payload,
            &exif.warnings,
        )
        .await?;
        sqlx::query(
            r"
            UPDATE node_metadata SET width = $2, height = $3, orientation = $4,
                camera_make = $5, camera_model = $6, has_gps = $7,
                gps_latitude = $8, gps_longitude = $9, updated_at = now()
            WHERE node_id = $1
            ",
        )
        .bind(job.target_node_id)
        .bind(exif.width)
        .bind(exif.height)
        .bind(exif.orientation)
        .bind(exif.camera_make)
        .bind(exif.camera_model)
        .bind(exif.gps_latitude.is_some() && exif.gps_longitude.is_some())
        .bind(exif.gps_latitude)
        .bind(exif.gps_longitude)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn extract_media(&self, job: &JobRecord, path: &Path) -> Result<()> {
        let probe = match extract_ffprobe(path).await {
            Ok(result) => result,
            Err(error) => return extractor_failed(&self.pool, job, "ffprobe", error).await,
        };
        persist_record(
            &self.pool,
            job.target_node_id,
            "ffprobe",
            "adapter-v1",
            "completed",
            &probe.raw_payload,
            &probe.warnings,
        )
        .await?;
        let streams = probe
            .streams
            .iter()
            .filter_map(stream_input)
            .collect::<Vec<_>>();
        replace_media_streams(&self.pool, job.target_node_id, &streams).await?;
        let video = probe
            .streams
            .iter()
            .find(|stream| stream.stream_type == StreamType::Video);
        sqlx::query(
            "UPDATE node_metadata SET duration_ms = $2, width = $3, height = $4, updated_at = now() WHERE node_id = $1",
        )
        .bind(job.target_node_id)
        .bind(probe.duration_ms)
        .bind(video.and_then(|stream| stream.width))
        .bind(video.and_then(|stream| stream.height))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn extract_document(&self, job: &JobRecord, path: &Path) -> Result<()> {
        let tika = match extract_tika(path, &self.tika_url).await {
            Ok(result) => result,
            Err(error) => return extractor_failed(&self.pool, job, "tika", error).await,
        };
        persist_record(
            &self.pool,
            job.target_node_id,
            "tika",
            "adapter-v1",
            "completed",
            &tika.raw_payload,
            &tika.warnings,
        )
        .await?;
        sqlx::query(
            r"
            UPDATE node_metadata SET page_count = $2, document_title = $3,
                document_author = $4, updated_at = now() WHERE node_id = $1
            ",
        )
        .bind(job.target_node_id)
        .bind(tika.page_count)
        .bind(tika.title)
        .bind(tika.author)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl JobHandler for MetadataHandler {
    async fn handle(&self, job: &JobRecord) -> Result<()> {
        match job.job_type {
            JobType::MetadataExtraction => self.extract(job).await,
            JobType::PreviewGeneration => self.generate_previews(job).await,
            _ => bail!("unsupported job type: {:?}", job.job_type),
        }
    }
}

async fn copy_to_path(storage: &dyn StorageBackend, storage_id: Uuid, path: &Path) -> Result<()> {
    let mut reader = storage.get_stream(StorageKey::original(storage_id)).await?;
    let mut file = tokio::fs::File::create(path)
        .await
        .context("create metadata extraction file")?;
    tokio::io::copy(&mut reader, &mut file)
        .await
        .context("copy original for metadata extraction")?;
    file.flush().await?;
    Ok(())
}

fn stream_input(stream: &strife_media::StreamInfo) -> Option<MediaStreamInput<'_>> {
    let stream_type = match stream.stream_type {
        StreamType::Video => MediaStreamType::Video,
        StreamType::Audio => MediaStreamType::Audio,
        StreamType::Subtitle => MediaStreamType::Subtitle,
        StreamType::Other => return None,
    };
    Some(MediaStreamInput {
        stream_index: stream.stream_index,
        stream_type,
        codec: &stream.codec,
        width: stream.width,
        height: stream.height,
        duration_ms: stream.duration_ms,
        bitrate_bps: stream.bitrate_bps,
        frame_rate: stream.frame_rate.as_deref(),
        language: stream.language.as_deref(),
    })
}

fn media_kind(mime: &str) -> &'static str {
    if mime.starts_with("image/") {
        "image"
    } else if mime.starts_with("video/") {
        "video"
    } else if mime.starts_with("audio/") {
        "audio"
    } else if is_document_mime(mime) {
        "document"
    } else {
        "other"
    }
}

fn is_document_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/pdf"
            | "application/msword"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    )
}

/// Content-MIME routing shared with the OCR handler after metadata detection.
pub(crate) fn is_ocr_candidate_mime(mime: &str) -> bool {
    strife_media::is_supported_ocr_mime(mime)
}

fn is_office_mime(mime: &str) -> bool {
    mime == "application/msword"
        || mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
}

async fn upsert_base_metadata(pool: &PgPool, node_id: Uuid, mime: &str) -> Result<()> {
    sqlx::query(
        r"
        INSERT INTO node_metadata (node_id, detected_mime, media_kind)
        VALUES ($1, $2, $3::media_kind)
        ON CONFLICT (node_id) DO UPDATE SET detected_mime = EXCLUDED.detected_mime,
            media_kind = EXCLUDED.media_kind, updated_at = now()
        ",
    )
    .bind(node_id)
    .bind(mime)
    .bind(media_kind(mime))
    .execute(pool)
    .await?;
    Ok(())
}

async fn persist_record(
    pool: &PgPool,
    node_id: Uuid,
    extractor_name: &str,
    extractor_version: &str,
    status: &str,
    raw_payload: &serde_json::Value,
    warnings: &[String],
) -> Result<()> {
    sqlx::query(
        r"
        INSERT INTO metadata_records (
            id, node_id, extractor_name, extractor_version, status, raw_payload, warnings
        ) VALUES ($1, $2, $3, $4, $5::metadata_status, $6, $7)
        ON CONFLICT (node_id, extractor_name) DO UPDATE SET
            extractor_version = EXCLUDED.extractor_version, status = EXCLUDED.status,
            raw_payload = EXCLUDED.raw_payload, warnings = EXCLUDED.warnings, updated_at = now()
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(extractor_name)
    .bind(extractor_version)
    .bind(status)
    .bind(raw_payload)
    .bind(warnings)
    .execute(pool)
    .await?;
    Ok(())
}

async fn extractor_failed(
    pool: &PgPool,
    job: &JobRecord,
    extractor_name: &str,
    error: anyhow::Error,
) -> Result<()> {
    let message = format!("{error:#}");
    persist_record(
        pool,
        job.target_node_id,
        extractor_name,
        "adapter-v1",
        "failed",
        &serde_json::json!({"error": message}),
        std::slice::from_ref(&message),
    )
    .await?;
    bail!(message)
}
