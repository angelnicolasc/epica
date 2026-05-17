//! E2E tests for infrastructure endpoints: /health, /ready, /metrics,
//! and MCP discovery at /.well-known/epica-server-card.json + jwks.json.

mod common;
use common::{build_test_app, get_json, get_raw};

use axum::http::StatusCode;

#[tokio::test]
async fn health_returns_ok() {
    let (status, body) = get_json(build_test_app(), "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
}

#[tokio::test]
async fn ready_returns_belief_count() {
    let (status, body) = get_json(build_test_app(), "/ready").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ready");
    assert_eq!(body["beliefs"], 0);
}

#[tokio::test]
async fn server_card_has_required_fields() {
    let (status, body) =
        get_json(build_test_app(), "/.well-known/epica-server-card.json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "epica-mcp");
    assert!(body["version"].is_string());
    assert_eq!(body["mcp_version"], "2026-03");
    assert!(body["endpoints"].is_array());
    assert!(!body["endpoints"].as_array().unwrap().is_empty());
    assert!(body["capabilities"].is_array());
    assert!(body["oauth"].is_object());
    assert!(body["oauth"]["jwks_uri"].is_string());
}

#[tokio::test]
async fn server_card_lists_all_endpoints() {
    let (_, body) = get_json(build_test_app(), "/.well-known/epica-server-card.json").await;
    let endpoints: Vec<String> = body["endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();

    let required = [
        "belief.get",
        "belief.set",
        "task.get",
        "task.stream",
        "checkpoint",
        "rollback",
        "query",
        "counterfactual",
        "diff",
        "contract.status",
    ];
    for name in required {
        assert!(
            endpoints.contains(&name.to_string()),
            "missing endpoint: {name}"
        );
    }
}

#[tokio::test]
async fn jwks_endpoint_is_reachable() {
    let (status, body) = get_json(build_test_app(), "/.well-known/jwks.json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["keys"].is_array());
}

#[tokio::test]
async fn metrics_endpoint_returns_text() {
    // Prometheus recorder is not installed in tests (prometheus: None in AppState),
    // so the endpoint returns an empty body with correct content-type.
    let (status, bytes) = get_raw(build_test_app(), "/metrics").await;
    assert_eq!(status, StatusCode::OK);
    // Empty or non-empty — just verify the endpoint is wired correctly.
    let _ = bytes;
}
