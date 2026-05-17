//! E2E tests for query, counterfactual, and contract status endpoints.

mod common;
use common::{build_test_app, get_json, post_json};

use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn query_empty_runtime_returns_empty_array() {
    let (status, body) = post_json(
        build_test_app(),
        "/v1/query",
        json!({ "query": "anything" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["beliefs"].is_array());
    assert!(body["beliefs"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn query_after_inserts_returns_results() {
    let app = build_test_app();

    for i in 0..3u32 {
        post_json(
            app.clone(),
            "/v1/beliefs",
            json!({ "key": format!("k{i}"), "value": format!("v{i}"), "confidence": 0.8 }),
        )
        .await;
    }

    let (status, body) = post_json(
        app,
        "/v1/query",
        json!({ "query": "v", "budget_tokens": 8192 }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let beliefs = body["beliefs"].as_array().unwrap();
    assert!(!beliefs.is_empty());

    // Each result has the expected fields
    let first = &beliefs[0];
    assert!(first["key"].is_string());
    assert!(first["fast_confidence"].is_number());
}

#[tokio::test]
async fn query_with_default_budget() {
    let app = build_test_app();
    post_json(
        app.clone(),
        "/v1/beliefs",
        json!({ "key": "singleton", "value": "only_one", "confidence": 1.0 }),
    )
    .await;

    // budget_tokens is optional — omitting it should use the 4096 default
    let (status, body) = post_json(app, "/v1/query", json!({ "query": "singleton" })).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["beliefs"].is_array());
}

#[tokio::test]
async fn counterfactual_unknown_key_returns_404() {
    let (status, body) = post_json(
        build_test_app(),
        "/v1/counterfactual",
        json!({ "belief_key": "ghost" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn counterfactual_returns_surviving_and_excluded_count() {
    let app = build_test_app();

    post_json(
        app.clone(),
        "/v1/beliefs",
        json!({ "key": "root", "value": "cause", "confidence": 0.9 }),
    )
    .await;

    let (status, body) = post_json(
        app,
        "/v1/counterfactual",
        json!({ "belief_key": "root" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["surviving"].is_array());
    assert!(body["excluded_count"].is_number());
}

#[tokio::test]
async fn contract_status_without_contract_returns_no_contract_message() {
    let (status, body) = get_json(build_test_app(), "/v1/contract/status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.get("status").is_some() || body.get("expected_drift").is_some(),
        "expected status or drift fields, got: {body}"
    );
}
