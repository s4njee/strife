//! Durable background-job processing for Strife.

use std::{env, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use sqlx::PgPool;
use strife_db::{
    JobRecord, JobType, claim_job, complete_job, enqueue_expired_trash_deletions, fail_job,
    get_job, release_expired_leases,
};
use strife_storage::StorageBackend;
use tokio::{sync::watch, task::JoinSet, time::MissedTickBehavior};
use tracing::{Instrument, error, info, info_span, warn};
use tracing_subscriber::EnvFilter;

mod deletion;
mod metadata;

pub use deletion::DeletionService;
pub use metadata::MetadataHandler;

/// Runtime settings loaded from environment variables.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerConfig {
    pub database_url: String,
    pub storage_root: PathBuf,
    pub tika_url: String,
    pub concurrency: usize,
    pub extractor_concurrency: usize,
    pub preview_concurrency: usize,
    pub poll_interval: Duration,
    pub lease_ttl: ChronoDuration,
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
            storage_root: PathBuf::from(required("STORAGE_ROOT")?),
            tika_url: required("TIKA_URL")?,
            concurrency: positive_usize("WORKER_CONCURRENCY", 2)?,
            extractor_concurrency: positive_usize("EXTRACTOR_CONCURRENCY", 1)?,
            preview_concurrency: positive_usize("PREVIEW_CONCURRENCY", 2)?,
            poll_interval: Duration::from_secs(positive_u64("WORKER_POLL_INTERVAL_SECONDS", 5)?),
            lease_ttl: ChronoDuration::seconds(i64::try_from(positive_u64(
                "WORKER_LEASE_TTL_SECONDS",
                300,
            )?)?),
        })
    }
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
                tika_url,
                extractor_concurrency,
                preview_concurrency,
            ),
            deletion: DeletionService::new(pool, storage),
        }
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
    tasks.spawn(lease_reaper(pool.clone(), shutdown_rx.clone()));
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
        let job = claim_next_job(&pool, &owner, config.lease_ttl).await?;
        match job {
            Some(job) => process_job(&pool, handler.as_ref(), job).await?,
            None => {
                tokio::select! {
                    () = tokio::time::sleep(config.poll_interval) => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                    }
                }
            }
        }
    }
}

async fn claim_next_job(
    pool: &PgPool,
    owner: &str,
    lease_ttl: ChronoDuration,
) -> Result<Option<JobRecord>> {
    for job_type in [
        JobType::PermanentDeletion,
        JobType::TrashCleanup,
        JobType::MetadataExtraction,
        JobType::PreviewGeneration,
    ] {
        if let Some(job) = claim_job(pool, job_type, owner, lease_ttl).await? {
            return Ok(Some(job));
        }
    }
    Ok(None)
}

async fn process_job(pool: &PgPool, handler: &dyn JobHandler, job: JobRecord) -> Result<()> {
    let span = info_span!("job", job_id = %job.id, job_type = ?job.job_type);
    async move {
        info!(attempt = job.attempts, "processing job");
        match handler.handle(&job).await {
            Ok(()) => {
                // Permanent deletion cascades jobs when the target node is removed.
                if get_job(pool, job.id).await?.is_some() {
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

#[cfg(not(unix))]
async fn wait_for_shutdown() -> Result<()> {
    tokio::signal::ctrl_c()
        .await
        .context("install Ctrl-C handler")
}
