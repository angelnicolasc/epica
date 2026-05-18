//! Shared test helpers for epica-mcp E2E tests.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use epica_core::BeliefQuad;
use epica_mcp::{build_router, middleware::build_rate_limiter, AppState, AuthConfig};
use epica_runtime::BeliefRuntime;

/// Build a test router with auth disabled and no Prometheus recorder.
pub fn build_test_app() -> Router {
    let runtime = Arc::new(BeliefRuntime::new(
        BeliefQuad::new(),
        0.5, // reliability_baseline
        10,  // system2_budget
        1.0, // system2_refill_rate
    ));
    let state = Arc::new(AppState {
        runtime,
        task_store: Box::new(epica_mcp::tasks::InMemoryTaskStore::new()),
        contract: None,
        auth: AuthConfig::disabled(),
        rate_limiter: build_rate_limiter(100_000), // very high limit — tests don't hit it
        prometheus: None,
    });
    build_router(state)
}

/// POST JSON body to the app and return (status, parsed JSON).
pub async fn post_json(
    app: Router,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    use tower::ServiceExt;
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// GET a path from the app and return (status, parsed JSON).
pub async fn get_json(app: Router, path: &str) -> (StatusCode, serde_json::Value) {
    use tower::ServiceExt;
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// GET a path and return (status, raw body bytes).
pub async fn get_raw(app: Router, path: &str) -> (StatusCode, axum::body::Bytes) {
    use tower::ServiceExt;
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, bytes)
}
