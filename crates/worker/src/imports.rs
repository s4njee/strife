use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail};
use strife_db::{JobRecord, JobType};
use strife_storage::{DiskGuard, StorageBackend};
use tracing::info;

/// Executes durable watched-folder scan jobs in the background worker.
pub struct ImportHandler {
    pool: sqlx::PgPool,
    storage: Arc<dyn StorageBackend>,
    storage_root: PathBuf,
    watch_root: PathBuf,
    disk_guard: DiskGuard,
}

impl ImportHandler {
    /// # Panics
    ///
    /// Panics when `disk_guard_percent` is outside the inclusive range 1..=100.
    #[must_use]
    pub fn new(
        pool: sqlx::PgPool,
        storage: Arc<dyn StorageBackend>,
        storage_root: PathBuf,
        watch_root: PathBuf,
        disk_guard_percent: u8,
    ) -> Self {
        Self {
            pool,
            storage,
            storage_root,
            watch_root,
            disk_guard: DiskGuard::new(disk_guard_percent)
                .expect("disk guard percentage must be between 1 and 100"),
        }
    }

    /// Runs the source scan represented by `job`.
    ///
    /// # Errors
    ///
    /// Returns an error when the job target is invalid or the resumable scan
    /// cannot complete.
    pub async fn scan(&self, job: &JobRecord) -> Result<()> {
        if job.job_type != JobType::ImportScan {
            bail!("import handler received unsupported job type");
        }
        let source_id = job
            .import_source_id
            .context("import scan job is missing its source")?;
        let source = strife_db::get_import_source(&self.pool, source_id)
            .await?
            .context("import scan source no longer exists")?;
        if !source.enabled {
            info!(%source_id, "skipping scan for disabled import source");
            return Ok(());
        }
        let report = strife_importer::run_import_scan(
            &self.pool,
            self.storage.as_ref(),
            &self.storage_root,
            &self.watch_root,
            &source,
            self.disk_guard,
        )
        .await?;
        info!(
            %source_id,
            discovered = report.scan.files_discovered,
            imported = report.imported,
            failed = report.failed,
            skipped_hidden = report.scan.hidden_entries_skipped,
            skipped_special = report.scan.special_entries_skipped,
            "import scan completed"
        );
        Ok(())
    }
}
