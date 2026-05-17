//! E2E tests for belief CRUD: insert, get, update, provenance, error paths.

mod common;
use common::{build_test_app, get_json, post_json};

use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn insert_new_belief_returns_belief_id() {
    let (status, body) = post_json(
        build_test_app(),
        "/v1/beliefs",
        json!({ "key": "user_intent", "value": "refactor auth", "confidence": 0.9 }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["belief_id"].is_string());
    assert!(!body["belief_id"].as_str().unwrap().is_empty());
    assert_eq!(body["system2_triggered"], false);
    assert!(body["task_id"].is_null());
}

#[tokio::test]
async fn get_inserted_belief_round_trips() {
    let app = build_test_app();

    post_json(
        app.clone(),
        "/v1/beliefs",
        json!({ "key": "blockers", "value": "missing test coverage", "confidence": 0.75 }),
    )
    .await;

    let (status, body) = get_json(app, "/v1/beliefs/blockers").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["key"], "blockers");
    assert!(body["value"].is_string());
    assert!((body["fast_confidence"].as_f64().unwrap() - 0.75).abs() < 1e-4);
    assert!(body["provenance"].is_string());
}

#[tokio::test]
async fn get_nonexistent_belief_returns_404() {
    let (status, body) = get_json(build_test_app(), "/v1/beliefs/does_not_exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn update_existing_belief_same_belief_id_family() {
    let app = build_test_app();

    let (_, first) = post_json(
        app.clone(),
        "/v1/beliefs",
        json!({ "key": "status", "value": "active", "confidence": 0.8 }),
    )
    .await;
    let first_id = first["belief_id"].as_str().unwrap().to_string();

    // Update the same key — should update in-place (same underlying SlotMap slot)
    let (status, second) = post_json(
        app,
        "/v1/beliefs",
        json!({ "key": "status", "value": "inactive", "confidence": 0.6 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The belief_id is the Debug-format of the SlotMap key; it stays the same slot.
    assert_eq!(second["belief_id"].as_str().unwrap(), first_id.as_str());
}

#[tokio::test]
async fn json_object_value_preserved() {
    let app = build_test_app();
    post_json(
        app.clone(),
        "/v1/beliefs",
        json!({
            "key": "metadata",
            "value": { "author": "test", "priority": 1 },
            "confidence": 0.95
        }),
    )
    .await;

    let (status, body) = get_json(app, "/v1/beliefs/metadata").await;
    assert_eq!(status, StatusCode::OK);
    // Value is stored as Asserted(string) — serialised JSON round-trips as a string
    assert!(body["value"].is_string());
}

#[tokio::test]
async fn provenance_kind_llm_sets_inference_provenance() {
    let app = build_test_app();
    post_json(
        app.clone(),
        "/v1/beliefs",
        json!({
            "key": "llm_belief",
            "value": "the answer is 42",
            "confidence": 0.7,
            "provenance_kind": "llm:claude-sonnet-4-6"
        }),
    )
    .await;

    let (status, body) = get_json(app, "/v1/beliefs/llm_belief").await;
    assert_eq!(status, StatusCode::OK);
    let prov = body["provenance"].as_str().unwrap();
    assert!(
        prov.contains("LlmInference") || prov.contains("Llm"),
        "expected LlmInference provenance, got: {prov}"
    );
}

#[tokio::test]
async fn multiple_beliefs_coexist() {
    let app = build_test_app();
    for i in 0..5u32 {
        post_json(
            app.clone(),
            "/v1/beliefs",
            json!({ "key": format!("belief_{i}"), "value": i, "confidence": 0.5 }),
        )
        .await;
    }

    // All five are retrievable
    for i in 0..5u32 {
        let (status, _) = get_json(app.clone(), &format!("/v1/beliefs/belief_{i}")).await;
        assert_eq!(status, StatusCode::OK, "belief_{i} not found");
    }
}

#[tokio::test]
async fn ready_reports_updated_belief_count() {
    let app = build_test_app();

    let (_, before) = get_json(app.clone(), "/ready").await;
    assert_eq!(before["beliefs"], 0);

    post_json(
        app.clone(),
        "/v1/beliefs",
        json!({ "key": "one", "value": "a", "confidence": 0.9 }),
    )
    .await;

    let (_, after) = get_json(app, "/ready").await;
    assert_eq!(after["beliefs"], 1);
}
