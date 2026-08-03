pub mod admin;
pub mod backfills;
pub mod config;
pub mod email;
pub mod email_parts;
pub mod files;
pub mod folders;
pub mod health;
pub mod imports;
pub mod jobs;
pub mod metadata;
pub mod nodes;
pub mod ocr;
pub mod search;
pub mod storage_usage;
pub mod uploads;

use std::{
    fs::{self, OpenOptions},
    future::IntoFuture,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use axum::http::StatusCode;
use config::Config;
use health::LiveDependencyChecker;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_storage::{DiskGuard, LocalFsBackend};
use tokio::{
    net::TcpListener,
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, timeout},
};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const DATABASE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum time to wait for in-flight HTTP work after a shutdown signal.
const HTTP_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time to wait for the upload-cleanup task after shutdown.
const CLEANUP_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const UPLOAD_CLEANUP_INTERVAL: Duration = Duration::from_secs(15 * 60);

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

/// Logs an internal failure and returns HTTP 500 without changing the client contract.
#[must_use]
pub fn internal_error(error: impl std::fmt::Display) -> StatusCode {
    error!(error = %error, "internal server error");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// Logs an internal failure and returns a typed API fallback (status/body unchanged).
#[must_use]
pub fn log_internal<T>(error: impl std::fmt::Display, fallback: T) -> T {
    error!(error = %error, "internal server error");
    fallback
}

/// Validates dependencies, applies migrations, and serves the API.
///
/// Listens until SIGTERM or Ctrl-C, then drains in-flight HTTP connections
/// (bounded) and stops the upload-session cleanup task.
///
/// # Errors
///
/// Returns a contextual error when storage, `PostgreSQL`, migrations, socket
/// binding, or the HTTP server cannot start.
pub async fn run(config: Config) -> Result<()> {
    verify_storage_root(&config.storage_root)?;
    let pool = connect_database(&config.database_url).await?;

    if config.run_migrations {
        strife_db::MIGRATOR
            .run(&pool)
            .await
            .context("failed to apply database migrations")?;
    }

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

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let cleanup_handle = spawn_upload_cleanup(pool.clone(), storage.clone(), shutdown_rx);

    let app = health::router(dependencies)
        .merge(admin::router(pool.clone()))
        .merge(backfills::router(pool.clone()))
        .merge(jobs::router(pool.clone()))
        .merge(metadata::router(pool.clone()))
        .merge(folders::router(pool.clone()))
        .merge(nodes::router(pool.clone()))
        .merge(email::router(pool.clone()))
        .merge(email_parts::router(pool.clone(), storage.clone()))
        .merge(ocr::router(pool.clone()))
        .merge(search::router(pool.clone()))
        .merge(storage_usage::router(pool.clone(), storage.clone()))
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

    let (signal_tx, signal_rx) = tokio::sync::oneshot::channel::<()>();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            if let Err(error) = wait_for_shutdown().await {
                warn!(%error, "shutdown signal handler failed");
            }
            info!("shutdown signal received; draining HTTP connections");
            let _ = shutdown_tx.send(true);
            let _ = signal_tx.send(());
        })
        .into_future();

    tokio::select! {
        result = server => {
            result.context("API server failed")?;
        }
        () = async {
            let _ = signal_rx.await;
            tokio::time::sleep(HTTP_DRAIN_TIMEOUT).await;
        } => {
            warn!(
                timeout_secs = HTTP_DRAIN_TIMEOUT.as_secs(),
                "HTTP drain timed out; forcing process exit path"
            );
        }
    }

    match timeout(CLEANUP_DRAIN_TIMEOUT, cleanup_handle).await {
        Ok(Ok(())) => info!("upload cleanup task joined"),
        Ok(Err(error)) => warn!(%error, "upload cleanup task panicked"),
        Err(_) => warn!(
            timeout_secs = CLEANUP_DRAIN_TIMEOUT.as_secs(),
            "upload cleanup did not stop within drain window"
        ),
    }

    info!("API shutdown complete");
    Ok(())
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

fn spawn_upload_cleanup(
    pool: PgPool,
    storage: Arc<LocalFsBackend>,
    shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(run_upload_cleanup(
        pool,
        storage,
        shutdown,
        UPLOAD_CLEANUP_INTERVAL,
    ))
}

/// Periodically purges expired upload sessions until `shutdown` is signalled.
async fn run_upload_cleanup(
    pool: PgPool,
    storage: Arc<LocalFsBackend>,
    mut shutdown: watch::Receiver<bool>,
    interval_period: Duration,
) {
    let mut interval = tokio::time::interval(interval_period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        if *shutdown.borrow() {
            info!("upload cleanup stopped");
            return;
        }
        tokio::select! {
            _ = interval.tick() => {
                match uploads::cleanup_expired_uploads(&pool, storage.as_ref()).await {
                    Ok(count) if count > 0 => {
                        info!(expired_uploads = count, "expired uploads cleaned");
                    }
                    Ok(_) => {}
                    Err(error) => warn!(%error, "expired upload cleanup failed"),
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    info!("upload cleanup stopped");
                    return;
                }
            }
        }
    }
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
    use std::{fs, path::PathBuf, sync::Arc, time::Duration};

    use axum::http::StatusCode;
    use tokio::{sync::watch, time::timeout};

    use super::{
        HTTP_DRAIN_TIMEOUT, internal_error, log_internal, run_upload_cleanup, verify_storage_root,
    };

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

    #[test]
    fn internal_error_maps_to_500_without_exposing_details() {
        let status = internal_error("db connection refused");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            log_internal("same cause", StatusCode::INTERNAL_SERVER_ERROR),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn drain_timeouts_are_bounded() {
        assert!(HTTP_DRAIN_TIMEOUT.as_secs() <= 60);
        assert!(super::CLEANUP_DRAIN_TIMEOUT.as_secs() <= 15);
    }

    #[tokio::test]
    async fn upload_cleanup_stops_promptly_on_shutdown_signal() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL unset; skipping upload cleanup shutdown test");
            return;
        };
        let Ok(pool) = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
        else {
            eprintln!("DATABASE_URL unreachable; skipping upload cleanup shutdown test");
            return;
        };
        let storage_root = temporary_path("cleanup-shutdown");
        fs::create_dir_all(&storage_root).expect("storage root");
        let storage = Arc::new(
            strife_storage::LocalFsBackend::new(&storage_root)
                .await
                .expect("storage"),
        );
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = tokio::spawn(run_upload_cleanup(
            pool,
            storage,
            shutdown_rx,
            Duration::from_secs(60),
        ));
        // Allow the task to enter the select loop before signalling.
        tokio::task::yield_now().await;
        shutdown_tx.send(true).expect("signal shutdown");
        timeout(Duration::from_secs(2), handle)
            .await
            .expect("cleanup must stop within drain window")
            .expect("cleanup task must not panic");
        let _ = fs::remove_dir_all(&storage_root);
    }
}
