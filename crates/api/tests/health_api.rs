use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use strife_api::health::{CheckFuture, DependencyChecker, StorageCheck};
use tower::ServiceExt;

#[derive(Clone, Copy)]
struct FixedChecks {
    postgres: bool,
    storage: bool,
    tika: bool,
}

impl DependencyChecker for FixedChecks {
    fn postgres(&self) -> CheckFuture<'_, bool> {
        Box::pin(async move { self.postgres })
    }

    fn storage(&self) -> CheckFuture<'_, StorageCheck> {
        Box::pin(async move { StorageCheck::new(self.storage, 37.5) })
    }

    fn tika(&self) -> CheckFuture<'_, bool> {
        Box::pin(async move { self.tika })
    }
}

async fn get(app: axum::Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::get(path).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("response body");
    (status, serde_json::from_slice(&bytes).expect("JSON body"))
}

#[tokio::test]
async fn public_health_routes_report_liveness_and_healthy_readiness() {
    let app = strife_api::health::router(FixedChecks {
        postgres: true,
        storage: true,
        tika: true,
    });

    let (status, body) = get(app.clone(), "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    let (status, body) = get(app, "/api/ready").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["postgres"], "ok");
    assert_eq!(body["storage"], "ok");
    assert_eq!(body["tika"], "ok");
    assert_eq!(body["disk_usage_percent"], 37.5);
}

#[tokio::test]
async fn readiness_identifies_a_degraded_dependency() {
    let app = strife_api::health::router(FixedChecks {
        postgres: true,
        storage: true,
        tika: false,
    });

    let (status, body) = get(app, "/api/ready").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["postgres"], "ok");
    assert_eq!(body["storage"], "ok");
    assert_eq!(body["tika"], "error");
}
