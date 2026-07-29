pub mod config;
pub mod files;
pub mod folders;
pub mod health;
pub mod imports;
pub mod uploads;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use config::Config;
use health::LiveDependencyChecker;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_storage::{DiskGuard, LocalFsBackend};
use tokio::{net::TcpListener, time::timeout};
use tracing::info;
use tracing::warn;
use tracing_subscriber::EnvFilter;

const DATABASE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Configures structured JSON logging for the API process.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .init();
}

/// Validates dependencies, applies migrations, and serves the API.
///
/// # Errors
///
/// Returns a contextual error when storage, `PostgreSQL`, migrations, socket
/// binding, or the HTTP server cannot start.
pub async fn run(config: Config) -> Result<()> {
    verify_storage_root(&config.storage_root)?;
    let pool = connect_database(&config.database_url).await?;

    strife_db::MIGRATOR
        .run(&pool)
        .await
        .context("failed to apply database migrations")?;

    let listener = TcpListener::bind(config.listen_addr)
        .await
        .with_context(|| format!("failed to bind API listener at {}", config.listen_addr))?;

    info!(
        listen_addr = %config.listen_addr,
        storage_root = %config.storage_root.display(),
        tika_url = %config.tika_url,
        "Strife API started"
    );

    let dependencies = LiveDependencyChecker::new(
        pool.clone(),
        config.storage_root.clone(),
        config.tika_url.clone(),
    );
    let storage = Arc::new(
        LocalFsBackend::new(&config.storage_root)
            .await
            .context("failed to initialize managed storage namespaces")?,
    );
    let upload_ttl = chrono::Duration::hours(
        i64::try_from(config.upload_session_ttl_hours)
            .context("UPLOAD_SESSION_TTL_HOURS is too large")?,
    );
    recover_watched_imports(&pool, storage.as_ref(), config.disk_guard_percent).await;
    spawn_upload_cleanup(pool.clone(), storage.clone());
    let app = health::router(dependencies)
        .merge(folders::router(pool.clone()))
        .merge(files::router(pool.clone(), storage.clone()))
        .merge(imports::router(
            pool.clone(),
            storage.clone(),
            config.storage_root.clone(),
            PathBuf::from("/mnt/ext/watch"),
            config.disk_guard_percent,
        ))
        .merge(uploads::router(
            pool,
            storage,
            upload_ttl,
            config.disk_guard_percent,
        ));

    axum::serve(listener, app)
        .await
        .context("API server failed")
}

async fn recover_watched_imports(pool: &PgPool, storage: &LocalFsBackend, disk_guard_percent: u8) {
    let watch_root = Path::new("/mnt/ext/watch");
    if !watch_root.is_dir() {
        return;
    }
    let Ok(Some(source)) =
        strife_db::get_import_source(pool, strife_db::DEFAULT_IMPORT_SOURCE_ID).await
    else {
        warn!("fixed import source could not be loaded for startup recovery");
        return;
    };
    let Some(guard) = DiskGuard::new(disk_guard_percent) else {
        warn!("invalid disk guard prevented import startup recovery");
        return;
    };
    match strife_importer::recover_interrupted_imports(
        pool,
        storage,
        watch_root,
        source.destination_folder_id,
        guard,
    )
    .await
    {
        Ok(report) if report.attempted > 0 => info!(
            attempted = report.attempted,
            completed = report.completed,
            failed = report.failures.len(),
            "interrupted imports recovered"
        ),
        Ok(_) => {}
        Err(error) => warn!(%error, "interrupted import recovery failed"),
    }
}

fn spawn_upload_cleanup(pool: PgPool, storage: Arc<LocalFsBackend>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15 * 60));
        loop {
            interval.tick().await;
            match uploads::cleanup_expired_uploads(&pool, storage.as_ref()).await {
                Ok(count) if count > 0 => info!(expired_uploads = count, "expired uploads cleaned"),
                Ok(_) => {}
                Err(error) => warn!(%error, "expired upload cleanup failed"),
            }
        }
    });
}

/// Connects to `PostgreSQL` within the startup deadline.
///
/// # Errors
///
/// Returns a timeout or connection error with startup context.
pub async fn connect_database(database_url: &str) -> Result<PgPool> {
    let connect = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(DATABASE_CONNECT_TIMEOUT)
        .connect(database_url);

    timeout(DATABASE_CONNECT_TIMEOUT, connect)
        .await
        .context("timed out connecting to PostgreSQL after 5 seconds")?
        .context("failed to connect to PostgreSQL")
}

/// Confirms that the configured storage root is a writable directory.
///
/// # Errors
///
/// Returns a contextual error when the path is absent, not a directory, or
/// cannot create and remove a probe file.
pub fn verify_storage_root(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("storage root {} is unavailable", path.display()))?;

    if !metadata.is_dir() {
        bail!("storage root {} is not a directory", path.display());
    }

    let probe_path = path.join(format!(".strife-write-check-{}", std::process::id()));
    let mut probe = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
        .with_context(|| format!("storage root {} is not writable", path.display()))?;

    probe
        .write_all(b"strife")
        .context("failed to write storage probe")?;
    drop(probe);
    fs::remove_file(&probe_path).context("failed to remove storage probe")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::verify_storage_root;

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "strife-api-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[test]
    fn accepts_a_writable_directory() {
        let path = temporary_path("writable");
        fs::create_dir_all(&path).expect("create test directory");

        let result = verify_storage_root(&path);

        fs::remove_dir_all(&path).expect("remove test directory");
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_a_missing_directory() {
        let path = temporary_path("missing");
        assert!(verify_storage_root(&path).is_err());
    }

    #[test]
    fn rejects_a_regular_file() {
        let path = temporary_path("file");
        fs::write(&path, b"not a directory").expect("create test file");

        let result = verify_storage_root(&path);

        fs::remove_file(&path).expect("remove test file");
        assert!(result.is_err());
    }
}
