pub mod admin;
pub mod backfills;
pub mod config;
pub mod email;
pub mod email_parts;
pub mod error;
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
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use axum::{
    body::Body,
    extract::{MatchedPath, State},
    http::{HeaderName, Method, Request, StatusCode, header},
    middleware::{Next, from_fn, from_fn_with_state},
    response::Response,
};
use config::Config;
use futures_util::{StreamExt, stream};
use health::LiveDependencyChecker;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_storage::{DiskGuard, LocalFsBackend};
use tokio::{
    net::TcpListener,
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, timeout},
};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const DATABASE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum time to wait for in-flight HTTP work after a shutdown signal.
const HTTP_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time to wait for the upload-cleanup task after shutdown.
const CLEANUP_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const UPLOAD_CLEANUP_INTERVAL: Duration = Duration::from_secs(15 * 60);
const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Clone, Copy)]
enum RequestLogLevel {
    Info,
    Debug,
}

#[derive(Clone, Default)]
struct DrainTracker(Arc<Mutex<DrainCounts>>);

#[derive(Default)]
struct DrainCounts {
    active: usize,
    completed: usize,
    shutdown_snapshot: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DrainSummary {
    active_at_signal: usize,
    drained: usize,
    not_drained: usize,
}

impl DrainTracker {
    fn start(&self) -> ActiveRequest {
        self.0.lock().expect("drain tracker").active += 1;
        ActiveRequest {
            tracker: self.clone(),
            finished: false,
        }
    }

    fn begin_shutdown(&self) {
        let mut counts = self.0.lock().expect("drain tracker");
        counts.shutdown_snapshot = Some((counts.active, counts.completed));
    }

    fn summary(&self) -> DrainSummary {
        let counts = self.0.lock().expect("drain tracker");
        let Some((active_at_signal, completed_at_signal)) = counts.shutdown_snapshot else {
            return DrainSummary::default();
        };
        let drained = counts
            .completed
            .saturating_sub(completed_at_signal)
            .min(active_at_signal);
        DrainSummary {
            active_at_signal,
            drained,
            not_drained: active_at_signal.saturating_sub(drained),
        }
    }
}

struct ActiveRequest {
    tracker: DrainTracker,
    finished: bool,
}

impl ActiveRequest {
    fn finish(mut self) {
        let mut counts = self.tracker.0.lock().expect("drain tracker");
        counts.active = counts.active.saturating_sub(1);
        counts.completed += 1;
        drop(counts);
        self.finished = true;
    }
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        if !self.finished {
            let mut counts = self.tracker.0.lock().expect("drain tracker");
            counts.active = counts.active.saturating_sub(1);
        }
    }
}

/// Configures structured JSON logging for the API process.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(true)
        .with_span_list(false)
        .init();
}

/// Adds one completion event and one correlation id to every HTTP request.
fn with_request_observability(app: axum::Router, drain_tracker: DrainTracker) -> axum::Router {
    app.layer(from_fn(mark_request_log_level))
        .layer(from_fn_with_state(drain_tracker, track_inflight_request))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<Body>| {
                    let matched_route = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map_or_else(|| request.uri().path(), MatchedPath::as_str);
                    let request_id = request
                        .extensions()
                        .get::<RequestId>()
                        .and_then(|id| id.header_value().to_str().ok())
                        .unwrap_or("missing");
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        path = %request.uri().path(),
                        matched_route,
                        request_id,
                    )
                })
                .on_response(
                    |response: &Response, latency: Duration, span: &tracing::Span| {
                        let _guard = span.enter();
                        let latency_ms = latency.as_secs_f64() * 1000.0;
                        match response
                            .extensions()
                            .get::<RequestLogLevel>()
                            .copied()
                            .unwrap_or(RequestLogLevel::Info)
                        {
                            RequestLogLevel::Info => {
                                tracing::info!(
                                    status = response.status().as_u16(),
                                    latency_ms,
                                    "request completed"
                                );
                            }
                            RequestLogLevel::Debug => {
                                tracing::debug!(
                                    status = response.status().as_u16(),
                                    latency_ms,
                                    "request completed"
                                );
                            }
                        }
                    },
                ),
        )
        .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
        .layer(SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid))
}

async fn track_inflight_request(
    State(tracker): State<DrainTracker>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let guard = tracker.start();
    let response = next.run(request).await;
    let (parts, body) = response.into_parts();
    let body = stream::unfold(
        (body.into_data_stream(), Some(guard)),
        |(mut body, mut guard)| async move {
            if let Some(item) = body.next().await {
                Some((item, (body, guard)))
            } else {
                if let Some(guard) = guard.take() {
                    guard.finish();
                }
                None
            }
        },
    );
    Response::from_parts(parts, Body::from_stream(body))
}

async fn mark_request_log_level(request: Request<Body>, next: Next) -> Response {
    let path = request.uri().path();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or(path, MatchedPath::as_str);
    let debug = matches!(path, "/health" | "/ready" | "/api/health" | "/api/ready")
        || request.headers().contains_key(header::RANGE)
        || (*request.method() == Method::PATCH && route.starts_with("/api/uploads/"))
        || route.ends_with("/download")
        || route.ends_with("/preview-native")
        || route.ends_with("/preview")
        || route.ends_with("/thumbnail")
        || route.contains("/parts/");
    let mut response = next.run(request).await;
    response.extensions_mut().insert(if debug {
        RequestLogLevel::Debug
    } else {
        RequestLogLevel::Info
    });
    response
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
        import_watch_root = %config.import_watch_root.display(),
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
    let import_watch_root = prepare_import_watch_root(&config, &pool, storage.as_ref()).await;

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
        .merge(files::router(pool.clone(), storage.clone()));
    let app = merge_import_router(
        app,
        &config,
        pool.clone(),
        storage.clone(),
        import_watch_root,
    );
    let app = app.merge(uploads::router(
        pool,
        storage,
        upload_ttl,
        config.disk_guard_percent,
    ));
    let drain_tracker = DrainTracker::default();
    let app = with_request_observability(app, drain_tracker.clone());

    let (signal_tx, signal_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown_tracker = drain_tracker.clone();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            if let Err(error) = wait_for_shutdown().await {
                warn!(%error, "shutdown signal handler failed");
            }
            shutdown_tracker.begin_shutdown();
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

    finish_shutdown(cleanup_handle, &drain_tracker).await;
    Ok(())
}

async fn finish_shutdown(
    cleanup_handle: tokio::task::JoinHandle<()>,
    drain_tracker: &DrainTracker,
) {
    match timeout(CLEANUP_DRAIN_TIMEOUT, cleanup_handle).await {
        Ok(Ok(())) => info!("upload cleanup task joined"),
        Ok(Err(error)) => warn!(%error, "upload cleanup task panicked"),
        Err(_) => warn!(
            timeout_secs = CLEANUP_DRAIN_TIMEOUT.as_secs(),
            "upload cleanup did not stop within drain window"
        ),
    }

    let drain = drain_tracker.summary();
    info!(
        requests_active_at_signal = drain.active_at_signal,
        requests_drained = drain.drained,
        requests_not_drained = drain.not_drained,
        "API shutdown complete"
    );
}

async fn prepare_import_watch_root(
    config: &Config,
    pool: &PgPool,
    storage: &LocalFsBackend,
) -> Option<PathBuf> {
    let watch_root = available_import_watch_root(&config.import_watch_root)?;
    recover_watched_imports(pool, storage, &watch_root, config.disk_guard_percent).await;
    Some(watch_root)
}

fn merge_import_router(
    app: axum::Router,
    config: &Config,
    pool: PgPool,
    storage: Arc<LocalFsBackend>,
    watch_root: Option<PathBuf>,
) -> axum::Router {
    match watch_root {
        Some(watch_root) => app.merge(imports::router(
            pool,
            storage,
            config.storage_root.clone(),
            watch_root,
            config.disk_guard_percent,
        )),
        None => app,
    }
}

fn available_import_watch_root(path: &Path) -> Option<PathBuf> {
    match fs::read_dir(path) {
        Ok(_) if path.is_dir() => Some(path.to_path_buf()),
        Ok(_) => {
            warn!(import_watch_root = %path.display(), "import watch root is not a directory; import routes disabled");
            None
        }
        Err(error) => {
            warn!(import_watch_root = %path.display(), %error, "import watch root is unavailable; import routes disabled");
            None
        }
    }
}

async fn recover_watched_imports(
    pool: &PgPool,
    storage: &LocalFsBackend,
    watch_root: &Path,
    disk_guard_percent: u8,
) {
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
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
        routing::get,
    };
    use tokio::{
        sync::{Notify, watch},
        time::timeout,
    };
    use tower::ServiceExt;
    use tracing::instrument::WithSubscriber;

    use super::{
        DrainSummary, DrainTracker, HTTP_DRAIN_TIMEOUT, available_import_watch_root,
        internal_error, log_internal, run_upload_cleanup, verify_storage_root,
        with_request_observability,
    };

    #[derive(Clone, Default)]
    struct LogWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("log buffer").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for LogWriter {
        type Writer = Self;

        fn make_writer(&'writer self) -> Self::Writer {
            self.clone()
        }
    }

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "strife-api-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[tokio::test]
    async fn request_tracing_reports_route_status_latency_and_request_id() {
        let writer = LogWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .without_time()
            .with_current_span(true)
            .with_writer(writer.clone())
            .finish();
        let app = with_request_observability(
            Router::new().route("/api/test/{id}", get(|| async { StatusCode::CREATED })),
            DrainTracker::default(),
        );
        let response = app
            .oneshot(
                Request::get("/api/test/42")
                    .body(Body::empty())
                    .expect("request"),
            )
            .with_subscriber(subscriber)
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .expect("request id response header");
        uuid::Uuid::parse_str(request_id).expect("UUID request id");

        let logs = String::from_utf8(writer.0.lock().expect("log buffer").clone())
            .expect("UTF-8 tracing output");
        for expected in [
            "request completed",
            "GET",
            "/api/test/42",
            "/api/test/{id}",
            "201",
            "latency_ms",
            request_id,
        ] {
            assert!(logs.contains(expected), "missing {expected}: {logs}");
        }
    }

    #[tokio::test]
    async fn probes_and_range_streams_are_silent_at_info() {
        let writer = LogWriter::default();
        let subscriber = tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .json()
                .without_time()
                .with_max_level(tracing::Level::INFO)
                .with_writer(writer.clone())
                .finish(),
        );
        let app = with_request_observability(
            Router::new()
                .route("/health", get(|| async { StatusCode::OK }))
                .route(
                    "/api/files/{id}/download",
                    get(|| async { StatusCode::PARTIAL_CONTENT }),
                ),
            DrainTracker::default(),
        );
        let health = app
            .clone()
            .oneshot(
                Request::get("/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .with_subscriber(subscriber.clone())
            .await
            .expect("health response");
        assert_eq!(health.status(), StatusCode::OK);
        let range = app
            .oneshot(
                Request::get("/api/files/42/download")
                    .header(header::RANGE, "bytes=0-9")
                    .body(Body::empty())
                    .expect("request"),
            )
            .with_subscriber(subscriber)
            .await
            .expect("range response");
        assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT);
        let logs = String::from_utf8(writer.0.lock().expect("log buffer").clone())
            .expect("UTF-8 tracing output");
        assert!(
            !logs.contains("request completed"),
            "unexpected info log: {logs}"
        );
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
    fn import_watch_root_is_enabled_only_for_a_readable_directory() {
        let directory = temporary_path("import-watch");
        fs::create_dir_all(&directory).expect("create import directory");
        assert_eq!(
            available_import_watch_root(&directory),
            Some(directory.clone())
        );
        fs::remove_dir_all(&directory).expect("remove import directory");
        assert!(available_import_watch_root(&directory).is_none());

        let file = temporary_path("import-watch-file");
        fs::write(&file, b"not a directory").expect("create import file");
        assert!(available_import_watch_root(&file).is_none());
        fs::remove_file(file).expect("remove import file");
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
    async fn an_inflight_stream_is_counted_as_drained_after_shutdown() {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let handler_entered = entered.clone();
        let handler_release = release.clone();
        let tracker = DrainTracker::default();
        let app = with_request_observability(
            Router::new().route(
                "/slow",
                get(move || {
                    let entered = handler_entered.clone();
                    let release = handler_release.clone();
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        "drained"
                    }
                }),
            ),
            tracker.clone(),
        );
        let request = tokio::spawn(async move {
            let response = app
                .oneshot(Request::get("/slow").body(Body::empty()).expect("request"))
                .await
                .expect("response");
            to_bytes(response.into_body(), 1_024)
                .await
                .expect("consume response")
        });

        entered.notified().await;
        tracker.begin_shutdown();
        release.notify_one();
        assert_eq!(request.await.expect("request task"), "drained");
        assert_eq!(
            tracker.summary(),
            DrainSummary {
                active_at_signal: 1,
                drained: 1,
                not_drained: 0,
            }
        );
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
