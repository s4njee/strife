use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::{task::spawn_blocking, time::timeout};

use crate::verify_storage_root;

const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

pub type CheckFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A serializable dependency state used by the readiness endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyStatus {
    Ok,
    Error,
}

/// Response body returned by the liveness endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthResponse {
    pub status: String,
}

/// Response body returned by the readiness endpoint.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReadinessResponse {
    pub postgres: DependencyStatus,
    pub storage: DependencyStatus,
    pub tika: DependencyStatus,
    pub disk_usage_percent: f64,
}

/// Dependency checks required for the API to accept traffic.
pub trait DependencyChecker: Send + Sync + 'static {
    fn postgres(&self) -> CheckFuture<'_, bool>;
    fn storage(&self) -> CheckFuture<'_, StorageCheck>;
    fn tika(&self) -> CheckFuture<'_, bool>;
}

/// Production dependency checker backed by real services and storage.
#[derive(Clone, Debug)]
pub struct LiveDependencyChecker {
    pool: PgPool,
    storage_root: PathBuf,
    tika_url: String,
    http_client: reqwest::Client,
}

impl LiveDependencyChecker {
    #[must_use]
    pub fn new(pool: PgPool, storage_root: PathBuf, tika_url: String) -> Self {
        Self {
            pool,
            storage_root,
            tika_url,
            http_client: reqwest::Client::new(),
        }
    }
}

impl DependencyChecker for LiveDependencyChecker {
    fn postgres(&self) -> CheckFuture<'_, bool> {
        Box::pin(async { strife_db::ping(&self.pool).await.is_ok() })
    }

    fn storage(&self) -> CheckFuture<'_, StorageCheck> {
        let storage_root = self.storage_root.clone();
        Box::pin(async move {
            spawn_blocking(move || check_storage(&storage_root))
                .await
                .unwrap_or_else(|_| StorageCheck::error())
        })
    }

    fn tika(&self) -> CheckFuture<'_, bool> {
        let version_url = format!("{}/version", self.tika_url.trim_end_matches('/'));
        Box::pin(async move {
            self.http_client
                .get(version_url)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
        })
    }
}

#[derive(Clone)]
struct AppState {
    dependencies: Arc<dyn DependencyChecker>,
}

/// Builds the API router with liveness and readiness endpoints.
pub fn router(dependencies: impl DependencyChecker) -> Router {
    router_from_arc(Arc::new(dependencies))
}

fn router_from_arc(dependencies: Arc<dyn DependencyChecker>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/api/health", get(health))
        .route("/api/ready", get(ready))
        .with_state(AppState { dependencies })
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
    })
}

async fn ready(State(state): State<AppState>) -> (StatusCode, Json<ReadinessResponse>) {
    let (postgres, storage, tika) = tokio::join!(
        timeout(CHECK_TIMEOUT, state.dependencies.postgres()),
        timeout(CHECK_TIMEOUT, state.dependencies.storage()),
        timeout(CHECK_TIMEOUT, state.dependencies.tika()),
    );

    let postgres = status(postgres.unwrap_or(false));
    let storage = storage.unwrap_or_else(|_| StorageCheck::error());
    let tika = status(tika.unwrap_or(false));
    let response = ReadinessResponse {
        postgres,
        storage: status(storage.healthy),
        tika,
        disk_usage_percent: storage.disk_usage_percent,
    };

    let response_status = if [response.postgres, response.storage, response.tika]
        .into_iter()
        .all(|dependency| dependency == DependencyStatus::Ok)
    {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (response_status, Json(response))
}

fn status(healthy: bool) -> DependencyStatus {
    if healthy {
        DependencyStatus::Ok
    } else {
        DependencyStatus::Error
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StorageCheck {
    healthy: bool,
    disk_usage_percent: f64,
}

impl StorageCheck {
    /// Creates a storage dependency result for custom dependency checkers.
    #[must_use]
    pub const fn new(healthy: bool, disk_usage_percent: f64) -> Self {
        Self {
            healthy,
            disk_usage_percent,
        }
    }

    const fn error() -> Self {
        Self::new(false, 100.0)
    }
}

fn check_storage(path: &Path) -> StorageCheck {
    if verify_storage_root(path).is_err() {
        return StorageCheck::error();
    }

    let Ok(total) = fs2::total_space(path) else {
        return StorageCheck::error();
    };
    let Ok(available) = fs2::available_space(path) else {
        return StorageCheck::error();
    };

    if total == 0 {
        return StorageCheck::error();
    }

    let used = total.saturating_sub(available);
    let permille = (u128::from(used) * 1_000) / u128::from(total);
    let permille = u32::try_from(permille).unwrap_or(1_000);
    let percentage = f64::from(permille) / 10.0;

    StorageCheck {
        healthy: true,
        disk_usage_percent: percentage.clamp(0.0, 100.0),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    use super::{
        CheckFuture, DependencyChecker, DependencyStatus, HealthResponse, ReadinessResponse,
        StorageCheck, router,
    };

    #[derive(Clone, Copy)]
    struct FixedChecks {
        postgres: bool,
        storage: bool,
        tika: bool,
        disk_usage_percent: f64,
    }

    impl DependencyChecker for FixedChecks {
        fn postgres(&self) -> CheckFuture<'_, bool> {
            Box::pin(async move { self.postgres })
        }

        fn storage(&self) -> CheckFuture<'_, StorageCheck> {
            Box::pin(async move {
                StorageCheck {
                    healthy: self.storage,
                    disk_usage_percent: self.disk_usage_percent,
                }
            })
        }

        fn tika(&self) -> CheckFuture<'_, bool> {
            Box::pin(async move { self.tika })
        }
    }

    #[tokio::test]
    async fn health_is_independent_of_dependencies() {
        let response = router(FixedChecks {
            postgres: false,
            storage: false,
            tika: false,
            disk_usage_percent: 100.0,
        })
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

        assert_eq!(response.status(), 200);
        let body = to_bytes(response.into_body(), 1_024).await.unwrap();
        let parsed: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            parsed,
            HealthResponse {
                status: "ok".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn ready_returns_ok_when_every_dependency_is_healthy() {
        let response = router(FixedChecks {
            postgres: true,
            storage: true,
            tika: true,
            disk_usage_percent: 61.2,
        })
        .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();

        assert_eq!(response.status(), 200);
        let body = to_bytes(response.into_body(), 1_024).await.unwrap();
        let parsed: ReadinessResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            parsed,
            ReadinessResponse {
                postgres: DependencyStatus::Ok,
                storage: DependencyStatus::Ok,
                tika: DependencyStatus::Ok,
                disk_usage_percent: 61.2,
            }
        );
    }

    #[tokio::test]
    async fn ready_returns_unavailable_with_per_dependency_errors() {
        let response = router(FixedChecks {
            postgres: false,
            storage: true,
            tika: false,
            disk_usage_percent: 42.0,
        })
        .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();

        assert_eq!(response.status(), 503);
        let body = to_bytes(response.into_body(), 1_024).await.unwrap();
        let parsed: ReadinessResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.postgres, DependencyStatus::Error);
        assert_eq!(parsed.storage, DependencyStatus::Ok);
        assert_eq!(parsed.tika, DependencyStatus::Error);
    }

    #[derive(Clone, Copy)]
    struct SlowPostgresCheck;

    impl DependencyChecker for SlowPostgresCheck {
        fn postgres(&self) -> CheckFuture<'_, bool> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                true
            })
        }

        fn storage(&self) -> CheckFuture<'_, StorageCheck> {
            Box::pin(async {
                StorageCheck {
                    healthy: true,
                    disk_usage_percent: 10.0,
                }
            })
        }

        fn tika(&self) -> CheckFuture<'_, bool> {
            Box::pin(async { true })
        }
    }

    #[tokio::test]
    async fn ready_times_out_stalled_checks_within_two_seconds() {
        let started = Instant::now();
        let response = router(SlowPostgresCheck)
            .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), 503);
        assert!(started.elapsed() < Duration::from_millis(2_500));
    }
}
