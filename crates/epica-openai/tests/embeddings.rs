//! Integration tests for [`OpenAiEmbeddingProvider`] using a `wiremock`
//! mock OpenAI embeddings server.
//!
//! These tests exercise the full provider end-to-end against a real HTTP
//! socket (no `reqwest` mock), so the production retry / batching / shape
//! handling is fully covered.

use std::sync::Arc;

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, EmbeddingProvider, Provenance};
use epica_openai::{
    OpenAiConfig, OpenAiEmbeddingConfig, OpenAiEmbeddingError, OpenAiEmbeddingProvider,
};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn embedding_response(items: &[(&str, Vec<f32>)]) -> ResponseTemplate {
    let data: Vec<_> = items
        .iter()
        .enumerate()
        .map(|(i, (_t, v))| {
            json!({
                "object": "embedding",
                "embedding": v,
                "index": i,
            })
        })
        .collect();
    let body = json!({
        "object": "list",
        "data": data,
        "model": "text-embedding-3-small",
        "usage": {"prompt_tokens": 10, "total_tokens": 10},
    });
    ResponseTemplate::new(200).set_body_json(body)
}

fn provider_against(server: &MockServer) -> OpenAiEmbeddingProvider {
    let cfg = OpenAiEmbeddingConfig::from_openai(
        OpenAiConfig::new("sk-test-1234").with_base_url(server.uri()),
    )
    .with_model("text-embedding-3-small")
    .with_batch_size(64);
    OpenAiEmbeddingProvider::new(cfg)
}

#[tokio::test]
async fn warm_async_populates_cache() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .and(header("authorization", "Bearer sk-test-1234"))
        .respond_with(embedding_response(&[
            ("alpha", vec![1.0, 0.0, 0.0]),
            ("beta", vec![0.0, 1.0, 0.0]),
        ]))
        .expect(1)
        .mount(&server)
        .await;

    let p = provider_against(&server);
    let fetched = p.warm_async(&["alpha", "beta"]).await.unwrap();
    assert_eq!(fetched, 2);
    assert_eq!(p.cached_len(), 2);
    assert_eq!(p.embed_cached("alpha"), Some(vec![1.0, 0.0, 0.0]));
    assert_eq!(p.embed_cached("beta"), Some(vec![0.0, 1.0, 0.0]));

    // Second call is a no-op: cache covers everything.
    let fetched = p.warm_async(&["alpha"]).await.unwrap();
    assert_eq!(fetched, 0);
}

#[tokio::test]
async fn warm_then_warm_async_drains_pending_queue() {
    // The sync warm() trait method queues; warm_async() must drain it.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(embedding_response(&[("queued", vec![0.5, 0.5])]))
        .expect(1)
        .mount(&server)
        .await;

    let p = provider_against(&server);
    p.warm(&["queued"]); // sync queue, no I/O
    assert_eq!(p.cached_len(), 0);

    let fetched = p.warm_async(&[]).await.unwrap();
    assert_eq!(fetched, 1);
    assert_eq!(p.embed_cached("queued"), Some(vec![0.5, 0.5]));
}

#[tokio::test]
async fn rate_limit_triggers_retry_then_succeeds() {
    let server = MockServer::start().await;

    // First call: 429. Second call: 200. wiremock matches in order via
    // `up_to_n_times`.
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(embedding_response(&[("retried", vec![0.7, 0.0])]))
        .expect(1)
        .mount(&server)
        .await;

    let p = provider_against(&server);
    let fetched = p.warm_async(&["retried"]).await.unwrap();
    assert_eq!(fetched, 1);
    assert_eq!(p.embed_cached("retried"), Some(vec![0.7, 0.0]));
}

#[tokio::test]
async fn auth_failure_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
        .expect(1)
        .mount(&server)
        .await;

    let p = provider_against(&server);
    let err = p.warm_async(&["x"]).await.unwrap_err();
    assert!(matches!(err, OpenAiEmbeddingError::ClientError(_)));
}

#[tokio::test]
async fn mismatched_response_count_surfaces_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(embedding_response(&[("only_one", vec![1.0])])) // expected 2
        .expect(1)
        .mount(&server)
        .await;

    let p = provider_against(&server);
    let err = p.warm_async(&["a", "b"]).await.unwrap_err();
    assert!(matches!(
        err,
        OpenAiEmbeddingError::Mismatch { expected: 2, got: 1 }
    ));
}

#[tokio::test]
async fn batches_split_large_input_lists() {
    let server = MockServer::start().await;
    // batch_size = 2 ⇒ 5 inputs ⇒ 3 calls.
    let cfg = OpenAiEmbeddingConfig::from_openai(
        OpenAiConfig::new("sk-test-1234").with_base_url(server.uri()),
    )
    .with_batch_size(2);
    let p = OpenAiEmbeddingProvider::new(cfg);

    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(embedding_response(&[
            ("t1", vec![1.0]),
            ("t2", vec![1.0]),
        ]))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(embedding_response(&[
            ("t3", vec![1.0]),
            ("t4", vec![1.0]),
        ]))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(embedding_response(&[("t5", vec![1.0])]))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let fetched = p
        .warm_async(&["t1", "t2", "t3", "t4", "t5"])
        .await
        .unwrap();
    assert_eq!(fetched, 5);
    assert_eq!(p.cached_len(), 5);
}

/// End-to-end: warm an `OpenAiEmbeddingProvider`, attach it to a
/// `BeliefQuad`, and verify that the K\*6 paraphrase short-circuit fires
/// when two cached texts have identical embeddings.
///
/// This is the "fully wired" check — TD-P8-001 is genuinely closed only if
/// the warmed provider talks correctly to `BeliefQuad`.
#[tokio::test]
async fn k6_semantic_paraphrase_works_against_warmed_provider() {
    let server = MockServer::start().await;
    // Two phrases that hash to identical embeddings ⇒ cosine = 1.0 ⇒
    // `EquivalenceVerdict::Equivalent`.
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(embedding_response(&[
            ("user wants to refactor authentication", vec![1.0, 0.0]),
            ("the user wants the auth subsystem refactored", vec![1.0, 0.0]),
        ]))
        .expect(1)
        .mount(&server)
        .await;

    let provider = Arc::new(provider_against(&server));
    provider
        .warm_async(&[
            "user wants to refactor authentication",
            "the user wants the auth subsystem refactored",
        ])
        .await
        .unwrap();

    let mut quad = BeliefQuad::new();
    quad.set_embedding_provider(provider as Arc<dyn EmbeddingProvider>);

    let id = quad.insert(BeliefNode::new(
        "user_intent",
        BeliefValue::Asserted("user wants to refactor authentication".into()),
        Provenance::UserStatement { turn: 0 },
        0.9,
    ));

    let record = quad
        .revise(
            id,
            BeliefValue::Asserted(
                "the user wants the auth subsystem refactored".into(),
            ),
            Provenance::UserStatement { turn: 1 },
            0.9,
        )
        .expect("revision succeeds");

    assert!(
        record.contracted.is_empty(),
        "K*6 paraphrase against the real provider must not trigger contraction"
    );
    assert!(record.postulate_audit.vacuity);
    assert!(record.postulate_audit.extensionality);
}
