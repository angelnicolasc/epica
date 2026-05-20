//! Bridge tests: confirm `OpenAiEmbeddingProvider` satisfies the
//! `epica_runtime::Embedder` async trait and that `OpenAiLlmClient` satisfies
//! `epica_runtime::ProspectiveClient`.
//!
//! These are tiny "the trait is implemented and behaves" checks, not
//! end-to-end LLM exercises. The HTTP wire shape is covered exhaustively by
//! `tests/embeddings.rs` and `tests/integration.rs`; this file's job is to
//! pin the trait bridges added in Sprint 5 so a future refactor that
//! accidentally drops the impl trips immediately.

use std::sync::Arc;

use epica_openai::{OpenAiConfig, OpenAiEmbeddingConfig, OpenAiEmbeddingProvider, OpenAiLlmClient};
use epica_runtime::{Embedder, ProspectiveClient};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn embedder_trait_uses_cache_hit_first() {
    // No HTTP server: a cache hit must short-circuit and never touch the wire.
    let cfg = OpenAiEmbeddingConfig::from_openai(OpenAiConfig::new("test-key"));
    let provider = OpenAiEmbeddingProvider::new(cfg);
    provider.insert_cached("warm text", vec![0.1, 0.2, 0.3]);

    let embedder: Arc<dyn Embedder> = Arc::new(provider);
    let v = embedder.embed("warm text").await.expect("cache hit must succeed");
    assert_eq!(v, vec![0.1, 0.2, 0.3]);
}

#[tokio::test]
async fn embedder_trait_falls_through_to_warm_async_on_miss() {
    // On a miss the bridge must call warm_async, populate the cache, and then
    // resolve. We point the provider at a wiremock server and assert one
    // request lands.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{
                "object": "embedding",
                "embedding": [0.5, 0.5, 0.0],
                "index": 0
            }],
            "model": "text-embedding-3-small",
            "usage": { "prompt_tokens": 1, "total_tokens": 1 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let cfg = OpenAiEmbeddingConfig::from_openai(
        OpenAiConfig::new("test-key").with_base_url(server.uri()),
    );
    let provider = OpenAiEmbeddingProvider::new(cfg);
    let embedder: Arc<dyn Embedder> = Arc::new(provider);

    let v = embedder.embed("cold text").await.expect("warm + read should succeed");
    assert_eq!(v, vec![0.5, 0.5, 0.0]);
}

#[tokio::test]
async fn prospective_client_trait_generates_scenarios() {
    // Pin the `ProspectiveClient` impl on `OpenAiLlmClient` against the
    // expected OpenAI function-call shape: forced tool_choice on
    // `generate_scenarios`, parsed back into `RawProspectiveScenario`s.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "generate_scenarios",
                            "arguments": "{\"scenarios\":[{\"query\":\"what auth library are we using?\"},{\"query\":\"who owns the auth module?\"}]}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = OpenAiLlmClient::new(
        OpenAiConfig::new("test-key").with_base_url(server.uri()),
    );
    let scenarios = client
        .generate_scenarios("auth_lib", "Asserted(\"using oauth2\")")
        .await
        .expect("scenario generation should succeed");

    assert_eq!(scenarios.len(), 2);
    assert!(scenarios[0].scenario.contains("auth library"));
    assert!(scenarios[0].causal_events.is_empty());
}
