use std::sync::Arc;

use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use strife_db::MIGRATOR;
use strife_media::verify_tesseract;
use strife_storage::LocalFsBackend;
use strife_worker::{OcrSettings, WorkerConfig, WorkerHandler, init_tracing, run};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let config = WorkerConfig::from_env()?;
    let max_connections = u32::try_from(config.concurrency)?.saturating_add(3);
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(&config.database_url)
        .await?;
    if config.run_migrations {
        MIGRATOR.run(&pool).await?;
    }
    let tesseract_version = verify_tesseract(&config.tesseract_binary).await?;
    strife_db::set_ocr_engine_state(&pool, "tesseract", &tesseract_version, &config.ocr_language)
        .await?;
    // The shared heavy-CPU permit is a set of durable slot rows, so its width is
    // reconciled at startup rather than baked into a migration. OCR, email
    // parsing, and attachment extraction all draw from it.
    let heavy_slots = strife_db::set_resource_slots(
        &pool,
        strife_db::JobResourceClass::HeavyCpu,
        i32::try_from(config.heavy_cpu_concurrency)?,
    )
    .await?;
    tracing::info!(
        heavy_cpu_slots = heavy_slots,
        "heavy CPU admission configured"
    );

    let storage = Arc::new(LocalFsBackend::new(&config.storage_root).await?);
    let handler = Arc::new(
        WorkerHandler::new(
            pool.clone(),
            storage.clone(),
            config.tika_url.clone(),
            config.extractor_concurrency,
            config.preview_concurrency,
        )
        .with_ocr_settings(OcrSettings {
            language: config.ocr_language.clone(),
            tesseract_binary: config.tesseract_binary.clone(),
            engine_version: tesseract_version,
            minimum_embedded_text_chars: config.minimum_embedded_text_chars,
            raster_dpi: config.ocr_raster_dpi,
            max_pages: config.ocr_max_pages,
            max_pixels_per_page: config.ocr_max_pixels_per_page,
            file_timeout: config.ocr_file_timeout,
            memory_limit_bytes: config.ocr_memory_limit_bytes,
            max_text_bytes: config.ocr_max_text_bytes,
        })
        .with_email_settings(strife_worker::EmailSettings {
            limits: config.email_limits,
            attachments: config.email_attachment_limits,
            file_timeout: config.email_file_timeout,
        })
        .with_imports(
            pool.clone(),
            storage.clone(),
            config.storage_root.clone(),
            config.watch_root.clone(),
            config.disk_guard_percent,
        ),
    );

    run(config, pool, storage, handler).await
}
