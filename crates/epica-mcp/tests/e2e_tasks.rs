//! E2E tests for the SEP-1686 Tasks primitive: poll endpoint and SSE stream.

mod common;
use common::{build_test_app, get_json, post_json};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn fetch_nonexistent_task_returns_404() {
    let task_id = uuid::Uuid::new_v4();
    let (status, body) =
        get_json(build_test_app(), &format!("/v1/tasks/{task_id}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn insert_belief_without_system2_has_no_task_id() {
    // With no LlmClient attached, System 2 will never activate → task_id = null.
    let (status, body) = post_json(
        build_test_app(),
        "/v1/beliefs",
        json!({ "key": "no_system2", "value": "data", "confidence": 0.5 }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["system2_triggered"], false);
    assert!(body["task_id"].is_null());
}

#[tokio::test]
async fn task_poll_endpoint_returns_correct_structure() {
    // We can't easily force System 2 activation without a mock LlmClient,
    // so we verify the 404 path produces the right error structure.
    let fake_id = "00000000-0000-0000-0000-000000000001";
    let (status, body) =
        get_json(build_test_app(), &format!("/v1/tasks/{fake_id}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    // Error message should mention the task id
    let error = body["error"].as_str().unwrap_or("");
    assert!(
        error.contains("task") || error.contains(fake_id),
        "error message did not reference task: {error}"
    );
}

#[tokio::test]
async fn task_stream_endpoint_is_reachable() {
    // The SSE stream endpoint should return a 200 for an unknown task
    // (the first event will be an error event, not a 404 status code,
    //  since SSE starts with 200 before the first event).
    let app = build_test_app();
    let fake_id = uuid::Uuid::new_v4();

    let req = Request::builder()
        .method("GET")
        .uri(&format!("/v1/tasks/{fake_id}/stream"))
        .header("Accept", "text/event-stream")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // SSE always opens with 200; the error is communicated as an event in the stream
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("text/event-stream"),
        "expected text/event-stream content-type, got: {ct}"
    );
}
