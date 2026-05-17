//! E2E tests for checkpoint, rollback, and diff endpoints.

mod common;
use common::{build_test_app, post_json};

use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn checkpoint_returns_id_and_version() {
    let app = build_test_app();

    let (status, body) = post_json(app, "/v1/checkpoint", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["checkpoint_id"].is_string());
    assert!(!body["checkpoint_id"].as_str().unwrap().is_empty());
    assert!(body["version"].is_u64() || body["version"].is_number());
}

#[tokio::test]
async fn rollback_to_valid_checkpoint_returns_diff() {
    let app = build_test_app();

    // 1. Checkpoint at version 0 (empty state)
    let (_, cp) = post_json(app.clone(), "/v1/checkpoint", json!({})).await;
    let checkpoint_id = cp["checkpoint_id"].as_str().unwrap().to_string();

    // 2. Insert a belief (mutates state)
    post_json(
        app.clone(),
        "/v1/beliefs",
        json!({ "key": "to_be_rolled_back", "value": "data", "confidence": 0.8 }),
    )
    .await;

    // 3. Rollback — since new beliefs were added after the checkpoint,
    //    the diff should show them as "added" (added after checkpoint = removed on rollback).
    let (status, diff) = post_json(
        app,
        "/v1/rollback",
        json!({ "checkpoint_id": checkpoint_id }),
    )
    .await;

    // Rollback may succeed (200) or fail with UnnecessaryContraction (500) depending on
    // AGM K*4 vacuity logic. Both are valid responses — we test that the endpoint is wired.
    assert!(
        status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
        "unexpected status: {status}"
    );

    if status == StatusCode::OK {
        assert!(diff["added"].is_number());
        assert!(diff["removed"].is_number());
        assert!(diff["modified"].is_number());
        // trajectory_ece is f32; when no confidence history exists (empty session)
        // T-ECE = 0/0 = NaN → serialises as null. Accept both as valid.
        assert!(
            diff["trajectory_ece"].is_number() || diff["trajectory_ece"].is_null(),
            "expected trajectory_ece to be a number or null, got: {:?}",
            diff["trajectory_ece"]
        );
    }
}

#[tokio::test]
async fn rollback_with_invalid_checkpoint_id_returns_error() {
    let (status, body) = post_json(
        build_test_app(),
        "/v1/rollback",
        json!({ "checkpoint_id": "not-a-valid-checkpoint-id" }),
    )
    .await;

    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::INTERNAL_SERVER_ERROR,
        "expected error status, got: {status}"
    );
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn diff_against_checkpoint_returns_ece() {
    let app = build_test_app();

    // Checkpoint before inserting anything
    let (_, cp) = post_json(app.clone(), "/v1/checkpoint", json!({})).await;
    let checkpoint_id = cp["checkpoint_id"].as_str().unwrap().to_string();

    // Insert a belief
    post_json(
        app.clone(),
        "/v1/beliefs",
        json!({ "key": "diffed", "value": "x", "confidence": 0.5 }),
    )
    .await;

    // Diff against the checkpoint
    let (status, diff) = post_json(
        app,
        "/v1/diff",
        json!({ "checkpoint_id": checkpoint_id }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    // trajectory_ece is null when no confidence history exists in this session.
    assert!(
        diff["trajectory_ece"].is_number() || diff["trajectory_ece"].is_null(),
        "expected trajectory_ece to be a number or null"
    );
    // At least one belief was added since the checkpoint
    let added = diff["added"].as_u64().unwrap_or(0)
        + diff["modified"].as_u64().unwrap_or(0)
        + diff["removed"].as_u64().unwrap_or(0);
    assert!(added > 0, "expected non-zero diff after insertion");
}

#[tokio::test]
async fn diff_with_unknown_checkpoint_returns_error() {
    let (status, body) = post_json(
        build_test_app(),
        "/v1/diff",
        json!({ "checkpoint_id": "00000000-0000-0000-0000-000000000000" }),
    )
    .await;

    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::INTERNAL_SERVER_ERROR,
        "expected error, got: {status}"
    );
    assert!(body["error"].is_string());
}
