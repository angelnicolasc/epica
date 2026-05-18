//! Integration tests for `OpenAiLlmClient` using a `wiremock` mock OpenAI server.
//!
//! Exercises three contracts:
//!   1. Happy path — a valid OpenAI tool-call response yields the expected
//!      `System2Result`.
//!   2. Retry on rate-limit — a 429 followed by a 200 returns the 200 payload
//!      after backoff (verifies the retry loop works against a real socket).
//!   3. Non-retryable error — a 401 (bad API key) is surfaced immediately.

use epica_openai::{OpenAiConfig, OpenAiLlmClient};
use epica_runtime::{DiagnosticSignal, LlmClient, LlmClientError};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn diagnostic() -> DiagnosticSignal {
    DiagnosticSignal {
        belief_key: "auth_intent".into(),
        fast_confidence: 0.92,
        reliability_baseline: 0.5,
        divergence: 0.42,
    }
}

fn ok_response(revised: f64, reasoning: &str) -> ResponseTemplate {
    let body = json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {
                        "name": "report_confidence",
                        // OpenAI returns arguments as a JSON-encoded string, not an object.
                        "arguments": serde_json::to_string(&json!({
                            "revised_confidence": revised,
                            "reasoning": reasoning,
                        })).unwrap(),
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });
    ResponseTemplate::new(200).set_body_json(body)
}

#[tokio::test]
async fn happy_path_returns_revised_confidence() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-test-1234"))
        .respond_with(ok_response(0.83, "model says recalibrate down"))
        .expect(1)
        .mount(&server)
        .await;

    let client = OpenAiLlmClient::new(
        OpenAiConfig::new("sk-test-1234").with_base_url(server.uri()),
    );

    let result = client.reflect(&diagnostic()).await.expect("happy-path call");
    assert!(
        (result.revised_confidence - 0.83).abs() < 1e-5,
        "expected 0.83, got {}",
        result.revised_confidence
    );
    assert_eq!(result.reasoning, "model says recalibrate down");
}

#[tokio::test]
async fn retries_after_rate_limit() {
    let server = MockServer::start().await;

    // First call: 429 → retryable.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    // Second call: 200 with valid payload.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ok_response(0.71, "recovered after backoff"))
        .expect(1)
        .mount(&server)
        .await;

    let client = OpenAiLlmClient::new(
        OpenAiConfig::new("sk-test-1234").with_base_url(server.uri()),
    );

    let result = client.reflect(&diagnostic()).await.expect("retry should succeed");
    assert!((result.revised_confidence - 0.71).abs() < 1e-5);
    assert_eq!(result.reasoning, "recovered after backoff");
}

#[tokio::test]
async fn does_not_retry_on_401() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
        // The retry loop classifies 4xx (other than 429) as terminal, so we
        // must observe exactly one call.
        .expect(1)
        .mount(&server)
        .await;

    let client = OpenAiLlmClient::new(
        OpenAiConfig::new("sk-bad").with_base_url(server.uri()),
    );

    let err = client.reflect(&diagnostic()).await.expect_err("401 must surface");
    // 4xx other than 429 must be classified as ClientError so the retry
    // policy does not waste budget on certain-to-fail repeats.
    assert!(matches!(err, LlmClientError::ClientError(_)), "got {err:?}");
}
