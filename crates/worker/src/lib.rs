//! Durable background-job processing for Strife.

use std::{env, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use sqlx::PgPool;
use strife_db::{
    JobRecord, JobType, claim_job_with_resource_lease, complete_job,
    enqueue_expired_trash_deletions, fail_job, get_job, release_expired_leases, renew_job_lease,
};
use strife_storage::StorageBackend;
use tokio::{sync::watch, task::JoinSet, time::MissedTickBehavior};
use tracing::{Instrument, error, info, info_span, warn};
use tracing_subscriber::EnvFilter;

mod backfill;
mod deletion;
mod email;
mod imports;
mod metadata;
mod ocr;

pub use backfill::{
    BackfillCandidateProvider, BackfillCoordinator, EmailBackfillProvider, OcrBackfillProvider,
};
pub use deletion::DeletionService;
pub use email::{EmailHandler, EmailSettings};
pub use imports::ImportHandler;
pub use metadata::MetadataHandler;
pub use ocr::{OcrHandler, OcrSettings};

/// Runtime settings loaded from environment variables.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerConfig {
    pub database_url: String,
    pub run_migrations: bool,
    pub backfill_enabled: bool,
    pub storage_root: PathBuf,
    pub tika_url: String,
    pub concurrency: usize,
    pub extractor_concurrency: usize,
    pub preview_concurrency: usize,
    pub watch_root: PathBuf,
    pub disk_guard_percent: u8,
    pub poll_interval: Duration,
    pub lease_ttl: ChronoDuration,
    pub minimum_embedded_text_chars: usize,
    pub ocr_language: String,
    pub tesseract_binary: String,
    pub ocr_raster_dpi: u32,
    pub ocr_max_pages: u32,
    pub ocr_max_pixels_per_page: u64,
    pub ocr_file_timeout: Duration,
    pub ocr_memory_limit_bytes: u64,
    pub ocr_max_text_bytes: usize,
}

impl WorkerConfig {
    /// Loads worker settings, applying conservative defaults for a 4 GB host.
    ///
    /// # Errors
    ///
    /// Returns an error for missing required settings or invalid numeric values.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            run_migrations: boolean("RUN_MIGRATIONS", true)?,
            backfill_enabled: boolean("BACKFILL_ENABLED", false)?,
            storage_root: PathBuf::from(required("STORAGE_ROOT")?),
            tika_url: required("TIKA_URL")?,
            concurrency: positive_usize("WORKER_CONCURRENCY", 2)?,
            extractor_concurrency: positive_usize("EXTRACTOR_CONCURRENCY", 1)?,
            preview_concurrency: positive_usize("PREVIEW_CONCURRENCY", 2)?,
            watch_root: PathBuf::from(
                env::var("IMPORT_WATCH_ROOT").unwrap_or_else(|_| "/mnt/ext/watch".into()),
            ),
            disk_guard_percent: percentage("DISK_GUARD_PERCENT", 90)?,
            poll_interval: Duration::from_secs(positive_u64("WORKER_POLL_INTERVAL_SECONDS", 5)?),
            lease_ttl: ChronoDuration::seconds(i64::try_from(positive_u64(
                "WORKER_LEASE_TTL_SECONDS",
                300,
            )?)?),
            minimum_embedded_text_chars: positive_usize("OCR_EMBEDDED_TEXT_MIN_CHARS", 20)?,
            ocr_language: env::var("STRIFE_OCR_LANGUAGE").unwrap_or_else(|_| "eng".to_owned()),
            tesseract_binary: env::var("TESSERACT_BIN").unwrap_or_else(|_| "tesseract".to_owned()),
            ocr_raster_dpi: u32::try_from(positive_u64("OCR_RASTER_DPI", 200)?)
                .context("OCR_RASTER_DPI is too large")?,
            ocr_max_pages: u32::try_from(positive_u64("OCR_MAX_PAGES", 100)?)
                .context("OCR_MAX_PAGES is too large")?,
            ocr_max_pixels_per_page: positive_u64("OCR_MAX_PIXELS_PER_PAGE", 40_000_000)?,
            ocr_file_timeout: Duration::from_secs(positive_u64("OCR_FILE_TIMEOUT_SECONDS", 600)?),
            ocr_memory_limit_bytes: positive_u64("OCR_MEMORY_LIMIT_BYTES", 512 * 1024 * 1024)?,
            ocr_max_text_bytes: positive_usize("OCR_MAX_TEXT_BYTES", 16 * 1024 * 1024)?,
        })
    }
}

fn boolean(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(value) => match value.as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" => Ok(false),
            _ => bail!("{name} must be true or false"),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("read {name}")),
    }
}

fn percentage(name: &str, default: u8) -> Result<u8> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse::<u8>()
            .with_context(|| format!("{name} must be an integer from 1 to 100"))?,
        Err(env::VarError::NotPresent) => default,
        Err(error) => return Err(error).with_context(|| format!("read {name}")),
    };
    if !(1..=100).contains(&value) {
        bail!("{name} must be between 1 and 100");
    }
    Ok(value)
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("{name} must be set"))
}

fn positive_u64(name: &str, default: u64) -> Result<u64> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("{name} must be a positive integer"))?,
        Err(env::VarError::NotPresent) => default,
        Err(error) => return Err(error).with_context(|| format!("read {name}")),
    };
    if value == 0 {
        bail!("{name} must be greater than zero");
    }
    Ok(value)
}

fn positive_usize(name: &str, default: usize) -> Result<usize> {
    usize::try_from(positive_u64(name, default as u64)?).context("worker setting is too large")
}

/// Processing boundary implemented by each supported job family.
#[async_trait]
pub trait JobHandler: Send + Sync {
    async fn handle(&self, job: &JobRecord) -> Result<()>;
}

/// Composite handler that routes metadata, preview, and permanent-deletion work.
pub struct WorkerHandler {
    metadata: MetadataHandler,
    deletion: DeletionService,
    imports: Option<ImportHandler>,
    ocr: OcrHandler,
    email: EmailHandler,
}

impl WorkerHandler {
    #[must_use]
    pub fn new(
        pool: PgPool,
        storage: Arc<dyn StorageBackend>,
        tika_url: String,
        extractor_concurrency: usize,
        preview_concurrency: usize,
    ) -> Self {
        Self {
            metadata: MetadataHandler::new(
                pool.clone(),
                storage.clone(),
                tika_url.clone(),
                extractor_concurrency,
                preview_concurrency,
            ),
            deletion: DeletionService::new(pool.clone(), storage.clone()),
            email: EmailHandler::new(pool.clone(), storage.clone()),
            ocr: OcrHandler::new(pool, storage, tika_url, 20),
            imports: None,
        }
    }

    /// Sets the global minimum used to distinguish usable embedded PDF text.
    #[must_use]
    pub fn with_embedded_text_minimum(mut self, minimum_chars: usize) -> Self {
        self.ocr.set_minimum_embedded_text_chars(minimum_chars);
        self
    }

    /// Applies the global email parsing limits and per-file timeout.
    #[must_use]
    pub fn with_email_settings(mut self, settings: EmailSettings) -> Self {
        self.email.set_settings(settings);
        self
    }

    /// Applies the verified global OCR engine and resource settings.
    #[must_use]
    pub fn with_ocr_settings(mut self, settings: OcrSettings) -> Self {
        self.ocr.set_settings(settings);
        self
    }

    /// Enables durable watched-folder scan processing for this worker.
    #[must_use]
    pub fn with_imports(
        mut self,
        pool: PgPool,
        storage: Arc<dyn StorageBackend>,
        storage_root: PathBuf,
        watch_root: PathBuf,
        disk_guard_percent: u8,
    ) -> Self {
        self.imports = Some(ImportHandler::new(
            pool,
            storage,
            storage_root,
            watch_root,
            disk_guard_percent,
        ));
        self
    }
}

#[async_trait]
impl JobHandler for WorkerHandler {
    async fn handle(&self, job: &JobRecord) -> Result<()> {
        match job.job_type {
            JobType::MetadataExtraction | JobType::PreviewGeneration => {
                self.metadata.handle(job).await
            }
            JobType::PermanentDeletion | JobType::TrashCleanup => self.deletion.purge(job).await,
            JobType::ImportScan => {
                self.imports
                    .as_ref()
                    .context("import scan processing is not configured")?
                    .scan(job)
                    .await
            }
            JobType::Ocr => self.ocr.handle(job).await,
            JobType::EmailExtraction => self.email.handle(job).await,
        }
    }
}

/// Initializes newline-delimited JSON logs using `RUST_LOG` when present.
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
}

/// Runs processors and lease recovery until SIGTERM or Ctrl-C, then drains active work.
///
/// # Errors
///
/// Returns an error when queue access, signal setup, or a worker task fails.
pub async fn run(
    config: WorkerConfig,
    pool: PgPool,
    _storage: Arc<dyn StorageBackend>,
    handler: Arc<dyn JobHandler>,
) -> Result<()> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut tasks = JoinSet::new();

    for processor_id in 0..config.concurrency {
        tasks.spawn(processor_loop(
            processor_id,
            config.clone(),
            pool.clone(),
            Arc::clone(&handler),
            shutdown_rx.clone(),
        ));
    }
    tasks.spawn(import_processor_loop(
        config.clone(),
        pool.clone(),
        Arc::clone(&handler),
        shutdown_rx.clone(),
    ));
    tasks.spawn(lease_reaper(pool.clone(), shutdown_rx.clone()));
    if config.backfill_enabled {
        tasks.spawn(backfill::coordinator_loop(
            pool.clone(),
            config.poll_interval,
            shutdown_rx.clone(),
        ));
    }
    tasks.spawn(trash_cleanup_loop(pool, shutdown_rx));

    wait_for_shutdown().await?;
    info!("shutdown requested; draining active jobs");
    let _ = shutdown_tx.send(true);
    while let Some(result) = tasks.join_next().await {
        result.context("worker task panicked")??;
    }
    Ok(())
}

async fn processor_loop(
    processor_id: usize,
    config: WorkerConfig,
    pool: PgPool,
    handler: Arc<dyn JobHandler>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let owner = format!("{}-{processor_id}", std::process::id());
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        let job = match claim_next_job(&pool, &owner, config.lease_ttl).await {
            Ok(job) => job,
            Err(error) => {
                error!(%error, "failed to claim background job");
                if wait_for_next_poll(&mut shutdown, config.poll_interval).await {
                    return Ok(());
                }
                continue;
            }
        };
        match job {
            Some(job) => {
                if let Err(error) = process_job(
                    &pool,
                    handler.as_ref(),
                    job.clone(),
                    lease_ttl_for(job.job_type, config.lease_ttl),
                )
                .await
                {
                    error!(%error, "background job processor failed");
                }
            }
            None => {
                if wait_for_next_poll(&mut shutdown, config.poll_interval).await {
                    return Ok(());
                }
            }
        }
    }
}

async fn import_processor_loop(
    config: WorkerConfig,
    pool: PgPool,
    handler: Arc<dyn JobHandler>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let owner = format!("{}-import", std::process::id());
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        let job = match claim_job_with_resource_lease(
            &pool,
            JobType::ImportScan,
            &owner,
            config.lease_ttl,
        )
        .await
        {
            Ok(job) => job,
            Err(error) => {
                error!(%error, "failed to claim import scan job");
                if wait_for_next_poll(&mut shutdown, config.poll_interval).await {
                    return Ok(());
                }
                continue;
            }
        };
        match job {
            Some(job) => {
                if let Err(error) =
                    process_job(&pool, handler.as_ref(), job, config.lease_ttl).await
                {
                    error!(%error, "import scan processor failed");
                }
            }
            None => {
                if wait_for_next_poll(&mut shutdown, config.poll_interval).await {
                    return Ok(());
                }
            }
        }
    }
}

async fn wait_for_next_poll(shutdown: &mut watch::Receiver<bool>, interval: Duration) -> bool {
    tokio::select! {
        () = tokio::time::sleep(interval) => false,
        changed = shutdown.changed() => changed.is_err() || *shutdown.borrow(),
    }
}

async fn claim_next_job(
    pool: &PgPool,
    owner: &str,
    lease_ttl: ChronoDuration,
) -> Result<Option<JobRecord>> {
    for job_type in PROCESSOR_CLAIM_ORDER {
        if let Some(job) =
            claim_job_with_resource_lease(pool, job_type, owner, lease_ttl_for(job_type, lease_ttl))
                .await?
        {
            return Ok(Some(job));
        }
    }
    Ok(None)
}

// Email parsing is claimed after metadata and preview work but before OCR:
// parsing MIME is cheaper than OCR and unlocks the Email tab sooner. Origin
// still dominates this ordering — foreground work outranks every job family.
const PROCESSOR_CLAIM_ORDER: [JobType; 6] = [
    JobType::PermanentDeletion,
    JobType::TrashCleanup,
    JobType::MetadataExtraction,
    JobType::PreviewGeneration,
    JobType::EmailExtraction,
    JobType::Ocr,
];

fn lease_ttl_for(job_type: JobType, base_ttl: ChronoDuration) -> ChronoDuration {
    match job_type {
        // OCR rasterizes and recognizes whole documents.
        JobType::Ocr => base_ttl * 3,
        // MIME parsing is bounded but a pathological message can still take
        // well beyond the base lease, so it gets headroom short of OCR's.
        JobType::EmailExtraction => base_ttl * 2,
        _ => base_ttl,
    }
}

async fn process_job(
    pool: &PgPool,
    handler: &dyn JobHandler,
    job: JobRecord,
    lease_ttl: ChronoDuration,
) -> Result<()> {
    let span = info_span!("job", job_id = %job.id, job_type = ?job.job_type);
    async move {
        info!(attempt = job.attempts, "processing job");
        match handle_with_lease_renewal(pool, handler, &job, lease_ttl).await {
            Ok(()) => {
                // Permanent deletion cascades jobs when the target node is removed.
                if get_job(pool, job.id)
                    .await?
                    .is_some_and(|record| record.state == strife_db::JobState::Leased)
                {
                    complete_job(pool, job.id).await?;
                }
                info!("job completed");
            }
            Err(error) => {
                let message = format!("{error:#}");
                if get_job(pool, job.id).await?.is_some() {
                    let updated = fail_job(pool, job.id, &message).await?;
                    warn!(error = %message, state = ?updated.map(|record| record.state), "job failed");
                } else {
                    warn!(error = %message, "job failed after target was removed");
                }
            }
        }
        Ok(())
    }
    .instrument(span)
    .await
}

async fn handle_with_lease_renewal(
    pool: &PgPool,
    handler: &dyn JobHandler,
    job: &JobRecord,
    lease_ttl: ChronoDuration,
) -> Result<()> {
    let owner = job
        .lease_owner
        .as_deref()
        .context("leased job is missing its owner")?;
    let renewal_millis = (lease_ttl.num_milliseconds() / 3).max(100);
    let mut interval = tokio::time::interval(Duration::from_millis(
        u64::try_from(renewal_millis).context("job lease renewal interval is invalid")?,
    ));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval.tick().await;

    let work = handler.handle(job);
    tokio::pin!(work);
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = interval.tick() => {
                if !renew_job_lease(pool, job.id, owner, lease_ttl).await? {
                    bail!("job lease was lost while work was still running");
                }
            }
        }
    }
}

async fn lease_reaper(pool: PgPool, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval.tick().await;
    loop {
        tokio::select! {
            _ = interval.tick() => match release_expired_leases(&pool).await {
                Ok(count) if count > 0 => info!(count, "released expired job leases"),
                Ok(_) => {}
                Err(error) => error!(%error, "failed to release expired job leases"),
            },
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return Ok(()); }
            }
        }
    }
}

const TRASH_CLEANUP_BATCH: u32 = 50;

/// Hourly sweep that queues permanent deletion for trash past its retention window.
async fn trash_cleanup_loop(pool: PgPool, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(3600));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // Run once shortly after startup so long-running hosts do not wait a full hour.
    interval.tick().await;
    loop {
        match enqueue_expired_trash_deletions(&pool, TRASH_CLEANUP_BATCH).await {
            Ok(count) if count > 0 => info!(count, "enqueued expired trash for permanent deletion"),
            Ok(_) => {}
            Err(error) => error!(%error, "failed to enqueue expired trash cleanup"),
        }
        tokio::select! {
            _ = interval.tick() => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return Ok(()); }
            }
        }
    }
}

/// Exposed for integration tests: run one trash-cleanup batch immediately.
///
/// # Errors
///
/// Returns a database error when enqueueing fails.
pub async fn run_trash_cleanup_once(pool: &PgPool) -> Result<u64> {
    Ok(enqueue_expired_trash_deletions(pool, TRASH_CLEANUP_BATCH).await?)
}

#[cfg(unix)]
async fn wait_for_shutdown() -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate()).context("install SIGTERM handler")?;
    tokio::select! {
        _ = terminate.recv() => {}
        result = tokio::signal::ctrl_c() => result.context("install Ctrl-C handler")?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Duration as ChronoDuration;
    use strife_db::JobType;

    use super::{PROCESSOR_CLAIM_ORDER, lease_ttl_for};

    #[test]
    fn ocr_is_claimed_after_interactive_work_with_a_longer_lease() {
        let ocr_position = PROCESSOR_CLAIM_ORDER
            .iter()
            .position(|job_type| *job_type == JobType::Ocr)
            .expect("OCR in claim order");
        let metadata_position = PROCESSOR_CLAIM_ORDER
            .iter()
            .position(|job_type| *job_type == JobType::MetadataExtraction)
            .expect("metadata in claim order");
        let preview_position = PROCESSOR_CLAIM_ORDER
            .iter()
            .position(|job_type| *job_type == JobType::PreviewGeneration)
            .expect("preview in claim order");
        assert!(ocr_position > metadata_position);
        assert!(ocr_position > preview_position);
        assert_eq!(
            lease_ttl_for(JobType::Ocr, ChronoDuration::minutes(5)),
            ChronoDuration::minutes(15)
        );
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() -> Result<()> {
    tokio::signal::ctrl_c()
        .await
        .context("install Ctrl-C handler")
}
